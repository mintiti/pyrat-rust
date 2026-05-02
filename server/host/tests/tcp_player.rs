//! Integration tests for [`TcpPlayer`] and [`accept_players`].
//!
//! Each test stands up a local TCP listener, runs the host-side
//! `accept_players` against it, and drives one or more "fake bot" clients
//! that speak the wire protocol via `pyrat_protocol::codec` and
//! `pyrat_wire::framing`. No SDK dependency — these tests exercise the host
//! side in isolation.

use std::time::Duration;

use pyrat::Direction;
use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::{accept_players, AcceptError, EventSink, Player, PlayerError, TcpPlayer};
use pyrat_protocol::{
    extract_host_msg, serialize_bot_msg, BotMsg, HostMsg, Info, OptionDef, OptionType,
};
use pyrat_wire::framing::{FrameReader, FrameWriter};
use pyrat_wire::{HostPacket, Player as PlayerSlot};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::timeout;

// ── Fake bot helper ───────────────────────────────────────────────────

/// Minimal bot-side driver. Connects, sends/receives owned messages over the
/// real wire framing + codec.
struct FakeBot {
    reader: FrameReader<tokio::net::tcp::OwnedReadHalf>,
    writer: FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
}

impl FakeBot {
    async fn connect(addr: std::net::SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).await.expect("bot connect");
        let _ = stream.set_nodelay(true);
        let (rx, tx) = stream.into_split();
        Self {
            reader: FrameReader::with_default_max(rx),
            writer: FrameWriter::with_default_max(tx),
        }
    }

    async fn send(&mut self, msg: BotMsg) {
        let bytes = serialize_bot_msg(&msg);
        self.writer.write_frame(&bytes).await.expect("bot write");
    }

    async fn recv(&mut self) -> HostMsg {
        let bytes = self.reader.read_frame().await.expect("bot read").to_vec();
        let packet = flatbuffers::root::<HostPacket>(&bytes).expect("decode HostPacket");
        extract_host_msg(&packet).expect("extract host msg")
    }

    async fn identify(&mut self, agent_id: &str) -> HostMsg {
        self.send(BotMsg::Identify {
            name: format!("fake/{agent_id}"),
            author: "tests".into(),
            agent_id: agent_id.into(),
            options: vec![],
        })
        .await;
        self.recv().await
    }
}

// ── Test utilities ────────────────────────────────────────────────────

async fn bound_listener() -> (TcpListener, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    (listener, addr)
}

fn expected_two() -> Vec<(PlayerSlot, String)> {
    vec![
        (PlayerSlot::Player1, "alice".into()),
        (PlayerSlot::Player2, "bob".into()),
    ]
}

// ── accept_players ────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accept_players_returns_two_in_slot_order() {
    let (listener, addr) = bound_listener().await;
    let host = tokio::spawn(async move {
        accept_players(
            &listener,
            &expected_two(),
            EventSink::noop(),
            Duration::from_secs(5),
        )
        .await
    });

    let mut alice = FakeBot::connect(addr).await;
    let mut bob = FakeBot::connect(addr).await;
    let alice_welcome = alice.identify("alice").await;
    let bob_welcome = bob.identify("bob").await;

    assert!(matches!(
        alice_welcome,
        HostMsg::Welcome {
            player_slot: PlayerSlot::Player1
        }
    ));
    assert!(matches!(
        bob_welcome,
        HostMsg::Welcome {
            player_slot: PlayerSlot::Player2
        }
    ));

    let players = host.await.unwrap().expect("accept_players ok");
    let [p1, p2] = players;
    let p1 = p1.expect("p1 present");
    let p2 = p2.expect("p2 present");
    assert_eq!(p1.identity().agent_id, "alice");
    assert_eq!(p1.identity().slot, PlayerSlot::Player1);
    assert_eq!(p2.identity().agent_id, "bob");
    assert_eq!(p2.identity().slot, PlayerSlot::Player2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accept_players_dispatches_by_agent_id_not_arrival_order() {
    // Bob connects first, but the slot-indexed return must put Alice in
    // position 0 (since expected[0] = (Player1, "alice")).
    let (listener, addr) = bound_listener().await;
    let host = tokio::spawn(async move {
        accept_players(
            &listener,
            &expected_two(),
            EventSink::noop(),
            Duration::from_secs(5),
        )
        .await
    });

    let mut bob = FakeBot::connect(addr).await;
    let _ = bob.identify("bob").await;
    let mut alice = FakeBot::connect(addr).await;
    let _ = alice.identify("alice").await;

    let [p1, p2] = host.await.unwrap().expect("accept_players ok");
    assert_eq!(p1.unwrap().identity().agent_id, "alice");
    assert_eq!(p2.unwrap().identity().agent_id, "bob");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accept_players_supports_single_slot_for_gui_mixed() {
    // GUI matches use one TcpPlayer + one EmbeddedPlayer. accept_players
    // accepts a length-1 expected and returns Some at the right slot.
    let (listener, addr) = bound_listener().await;
    let expected = vec![(PlayerSlot::Player2, "solo".into())];
    let host = tokio::spawn(async move {
        accept_players(
            &listener,
            &expected,
            EventSink::noop(),
            Duration::from_secs(5),
        )
        .await
    });

    let mut bot = FakeBot::connect(addr).await;
    let welcome = bot.identify("solo").await;
    assert!(matches!(
        welcome,
        HostMsg::Welcome {
            player_slot: PlayerSlot::Player2
        }
    ));

    let [p1, p2] = host.await.unwrap().expect("accept_players ok");
    assert!(p1.is_none(), "Player1 slot should be None");
    assert_eq!(p2.unwrap().identity().slot, PlayerSlot::Player2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accept_players_rejects_unknown_agent_id_and_keeps_waiting() {
    let (listener, addr) = bound_listener().await;
    let host = tokio::spawn(async move {
        accept_players(
            &listener,
            &expected_two(),
            EventSink::noop(),
            Duration::from_secs(5),
        )
        .await
    });

    // A stranger connects first — should receive ProtocolError, get dropped,
    // accept_players keeps waiting.
    let mut stranger = FakeBot::connect(addr).await;
    let stranger_reply = stranger.identify("stranger").await;
    assert!(
        matches!(stranger_reply, HostMsg::ProtocolError { ref reason } if reason.contains("unknown agent_id")),
        "expected ProtocolError, got {stranger_reply:?}"
    );

    // Valid bots still complete.
    let mut alice = FakeBot::connect(addr).await;
    let _ = alice.identify("alice").await;
    let mut bob = FakeBot::connect(addr).await;
    let _ = bob.identify("bob").await;

    let [p1, p2] = host.await.unwrap().expect("accept_players ok");
    assert_eq!(p1.unwrap().identity().agent_id, "alice");
    assert_eq!(p2.unwrap().identity().agent_id, "bob");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accept_players_rejects_duplicate_agent_id() {
    let (listener, addr) = bound_listener().await;
    let host = tokio::spawn(async move {
        accept_players(
            &listener,
            &expected_two(),
            EventSink::noop(),
            Duration::from_secs(5),
        )
        .await
    });

    let mut alice1 = FakeBot::connect(addr).await;
    let _ = alice1.identify("alice").await;
    // Second alice → ProtocolError, dropped.
    let mut alice2 = FakeBot::connect(addr).await;
    let alice2_reply = alice2.identify("alice").await;
    assert!(
        matches!(alice2_reply, HostMsg::ProtocolError { ref reason } if reason.contains("already claimed")),
        "expected duplicate rejection, got {alice2_reply:?}"
    );

    // Bob completes the match.
    let mut bob = FakeBot::connect(addr).await;
    let _ = bob.identify("bob").await;

    let [p1, p2] = host.await.unwrap().expect("accept_players ok");
    assert!(p1.is_some());
    assert!(p2.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accept_players_head_of_line_resilient_against_silent_socket() {
    // A ghost connects but never sends Identify. Its handshake task times
    // out alone (per-connection deadline) without blocking valid bots.
    let (listener, addr) = bound_listener().await;
    let host = tokio::spawn(async move {
        accept_players(
            &listener,
            &expected_two(),
            EventSink::noop(),
            Duration::from_secs(10),
        )
        .await
    });

    let _ghost = TcpStream::connect(addr).await.expect("ghost connect");

    // Valid bots still go through within a reasonable bound.
    let mut alice = FakeBot::connect(addr).await;
    let _ = alice.identify("alice").await;
    let mut bob = FakeBot::connect(addr).await;
    let _ = bob.identify("bob").await;

    let result = timeout(Duration::from_secs(8), host)
        .await
        .expect("host completed before overall timeout")
        .expect("task didn't panic")
        .expect("accept_players ok");
    assert!(result[0].is_some());
    assert!(result[1].is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_players_hivemind_rejected_immediately() {
    let (listener, _) = bound_listener().await;
    let expected = vec![
        (PlayerSlot::Player1, "twin".into()),
        (PlayerSlot::Player2, "twin".into()),
    ];
    let result = accept_players(
        &listener,
        &expected,
        EventSink::noop(),
        Duration::from_secs(1),
    )
    .await;
    match result {
        Err(AcceptError::HivemindNotSupported(_)) => {},
        Err(other) => panic!("expected HivemindNotSupported, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_players_invalid_expected_rejected() {
    let (listener, _) = bound_listener().await;
    let result = accept_players(&listener, &[], EventSink::noop(), Duration::from_secs(1)).await;
    match result {
        Err(AcceptError::InvalidExpected(_)) => {},
        Err(other) => panic!("expected InvalidExpected, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_players_rejects_duplicate_slot_in_expected() {
    // Two distinct agent_ids both targeting Player1 is invalid input — both
    // would race for the same slot. Reject at validation time rather than
    // letting one connection time out awkwardly.
    let (listener, _) = bound_listener().await;
    let expected = vec![
        (PlayerSlot::Player1, "alice".into()),
        (PlayerSlot::Player1, "bob".into()),
    ];
    let result = accept_players(
        &listener,
        &expected,
        EventSink::noop(),
        Duration::from_secs(1),
    )
    .await;
    match result {
        Err(AcceptError::InvalidExpected(reason)) => {
            assert!(
                reason.contains("slot") && reason.contains("twice"),
                "reason should mention duplicate slot, got: {reason}"
            );
        },
        Err(other) => panic!("expected InvalidExpected, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_players_overall_timeout_fires() {
    // No bots connect; the overall timeout fires.
    let (listener, _) = bound_listener().await;
    let result = accept_players(
        &listener,
        &expected_two(),
        EventSink::noop(),
        Duration::from_millis(200),
    )
    .await;
    match result {
        Err(AcceptError::Timeout) => {},
        Err(other) => panic!("expected Timeout, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

// ── TcpPlayer behaviour ───────────────────────────────────────────────

/// Set up a single TcpPlayer (Player1 slot) wired to a FakeBot. Returns the
/// TcpPlayer and the FakeBot so the test can drive both sides.
async fn one_tcp_player(event_sink: EventSink) -> (TcpPlayer, FakeBot) {
    let (listener, addr) = bound_listener().await;
    let expected = vec![(PlayerSlot::Player1, "alice".into())];
    let host = tokio::spawn(async move {
        accept_players(&listener, &expected, event_sink, Duration::from_secs(5)).await
    });
    let mut bot = FakeBot::connect(addr).await;
    let _ = bot.identify("alice").await;
    let [p1, _] = host.await.unwrap().expect("accept_players ok");
    (p1.expect("p1 present"), bot)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_player_round_trip_messages() {
    let (mut player, mut bot) = one_tcp_player(EventSink::noop()).await;

    // Host → bot: Configure (we don't actually need a valid config for this
    // test, just any HostMsg that survives serialize/parse round-trip).
    player
        .send(HostMsg::GoPreprocess { state_hash: 0xDEAD })
        .await
        .unwrap();
    let received = bot.recv().await;
    assert!(
        matches!(received, HostMsg::GoPreprocess { state_hash } if state_hash == 0xDEAD),
        "{received:?}"
    );

    // Bot → host: PreprocessingDone. recv() returns it.
    bot.send(BotMsg::PreprocessingDone).await;
    let got = timeout(Duration::from_secs(2), player.recv())
        .await
        .expect("recv timeout")
        .expect("recv ok")
        .expect("recv some");
    assert!(matches!(got, BotMsg::PreprocessingDone), "{got:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_player_provisional_stored_and_emitted_to_event_sink() {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let (mut player, mut bot) = one_tcp_player(EventSink::new(events_tx)).await;

    bot.send(BotMsg::Provisional {
        direction: Direction::Right,
        player: PlayerSlot::Player1,
        turn: 5,
        state_hash: 0xCAFE,
    })
    .await;
    bot.send(BotMsg::Action {
        direction: Direction::Stay,
        player: PlayerSlot::Player1,
        turn: 5,
        state_hash: 0xCAFE,
        think_ms: 1,
    })
    .await;

    // recv() returns Action — Provisional intercepted.
    let got = timeout(Duration::from_secs(2), player.recv())
        .await
        .expect("recv timeout")
        .expect("recv ok")
        .expect("recv some");
    assert!(matches!(got, BotMsg::Action { .. }), "{got:?}");

    // EventSink got BotProvisional.
    let mut got_provisional = false;
    while let Ok(event) = events_rx.try_recv() {
        if let MatchEvent::BotProvisional {
            sender,
            direction,
            turn,
            state_hash,
        } = event
        {
            assert_eq!(sender, PlayerSlot::Player1);
            assert_eq!(direction, Direction::Right);
            assert_eq!(turn, 5);
            assert_eq!(state_hash, 0xCAFE);
            got_provisional = true;
        }
    }
    assert!(got_provisional, "expected BotProvisional event");

    // take_provisional matches turn+hash, then is empty.
    assert_eq!(player.take_provisional(5, 0xCAFE), Some(Direction::Right));
    assert_eq!(player.take_provisional(5, 0xCAFE), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_player_provisional_cleared_on_next_go() {
    let (mut player, mut bot) = one_tcp_player(EventSink::noop()).await;

    bot.send(BotMsg::Provisional {
        direction: Direction::Up,
        player: PlayerSlot::Player1,
        turn: 5,
        state_hash: 0xAAAA,
    })
    .await;
    // Pump recv so the Provisional is pulled into TcpPlayer's storage. We
    // need a follow-up message because Provisional is filtered; send Action
    // and recv that.
    bot.send(BotMsg::Action {
        direction: Direction::Stay,
        player: PlayerSlot::Player1,
        turn: 5,
        state_hash: 0xAAAA,
        think_ms: 1,
    })
    .await;
    let _ = timeout(Duration::from_secs(2), player.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    // Sending Go clears the slot (whole-turn boundary).
    player
        .send(HostMsg::Go {
            state_hash: 0xBBBB,
            limits: pyrat_protocol::SearchLimits::default(),
        })
        .await
        .unwrap();
    assert_eq!(player.take_provisional(5, 0xAAAA), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_player_rejects_action_with_wrong_slot() {
    let (mut player, mut bot) = one_tcp_player(EventSink::noop()).await;

    // Player1 connection sends an Action tagged for Player2 → ProtocolError.
    bot.send(BotMsg::Action {
        direction: Direction::Stay,
        player: PlayerSlot::Player2,
        turn: 0,
        state_hash: 0,
        think_ms: 1,
    })
    .await;
    let err = timeout(Duration::from_secs(2), player.recv())
        .await
        .expect("recv timeout")
        .expect_err("expected ProtocolError");
    assert!(
        matches!(err, PlayerError::ProtocolError(ref msg) if msg.contains("wrong slot") || msg.contains("Player2")),
        "{err:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_player_info_routes_to_event_sink() {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let (mut player, mut bot) = one_tcp_player(EventSink::new(events_tx)).await;

    bot.send(BotMsg::Info(Info {
        player: PlayerSlot::Player1,
        multipv: 1,
        target: None,
        depth: 3,
        nodes: 100,
        score: Some(0.5),
        pv: vec![],
        message: "hello".into(),
        turn: 7,
        state_hash: 0xDEAD,
    }))
    .await;
    // Send a follow-up game-driving message so recv() has something to return.
    bot.send(BotMsg::PreprocessingDone).await;
    let got = timeout(Duration::from_secs(2), player.recv())
        .await
        .expect("recv timeout")
        .expect("recv ok")
        .expect("recv some");
    assert!(matches!(got, BotMsg::PreprocessingDone), "{got:?}");

    // Info landed in the event sink.
    let mut got_info = false;
    while let Ok(event) = events_rx.try_recv() {
        if let MatchEvent::BotInfo { sender, info, .. } = event {
            assert_eq!(sender, PlayerSlot::Player1);
            assert_eq!(info.message, "hello");
            got_info = true;
        }
    }
    assert!(got_info, "expected BotInfo in event sink");
}

/// Info is observer-facing analysis sideband — `info.player` is the analysis
/// subject, not a sender claim. A bot may legitimately analyze the opponent's
/// position. Cross-slot Info must reach the event sink without erroring `recv`.
/// Regression: slice 4's `check_slot` over-applied the rule to sideband.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_player_info_with_other_slot_routes_to_event_sink() {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let (mut player, mut bot) = one_tcp_player(EventSink::new(events_tx)).await;

    // Player1 connection sends Info tagged for Player2 (analysis of opponent).
    bot.send(BotMsg::Info(Info {
        player: PlayerSlot::Player2,
        multipv: 1,
        target: None,
        depth: 3,
        nodes: 100,
        score: Some(0.5),
        pv: vec![],
        message: "opp analysis".into(),
        turn: 7,
        state_hash: 0xDEAD,
    }))
    .await;
    bot.send(BotMsg::PreprocessingDone).await;
    let got = timeout(Duration::from_secs(2), player.recv())
        .await
        .expect("recv timeout")
        .expect("recv ok")
        .expect("recv some");
    assert!(matches!(got, BotMsg::PreprocessingDone), "{got:?}");

    let mut got_info = false;
    while let Ok(event) = events_rx.try_recv() {
        if let MatchEvent::BotInfo { sender, info, .. } = event {
            // sender = connection's slot; info.player = analysis subject.
            assert_eq!(sender, PlayerSlot::Player1);
            assert_eq!(info.player, PlayerSlot::Player2);
            assert_eq!(info.message, "opp analysis");
            got_info = true;
        }
    }
    assert!(got_info, "expected BotInfo in event sink");
}

/// RenderCommands is observer-facing sideband (same classification as Info).
/// Cross-slot must not error `recv`. Today the event isn't yet wired to a
/// `MatchEvent::BotRenderCommands` variant, so the asserted shape is just
/// "`recv` returns the next game-driving message cleanly."
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_player_render_commands_with_other_slot_does_not_error() {
    let (mut player, mut bot) = one_tcp_player(EventSink::noop()).await;

    bot.send(BotMsg::RenderCommands {
        player: PlayerSlot::Player2,
        turn: 5,
        state_hash: 0xBEEF,
    })
    .await;
    bot.send(BotMsg::PreprocessingDone).await;
    let got = timeout(Duration::from_secs(2), player.recv())
        .await
        .expect("recv timeout")
        .expect("recv ok")
        .expect("recv some");
    assert!(matches!(got, BotMsg::PreprocessingDone), "{got:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_player_close_drops_session_task() {
    let (player, mut bot) = one_tcp_player(EventSink::noop()).await;

    Box::new(player).close().await.expect("close ok");
    // After close, the bot's read should observe a peer disconnect (FIN).
    // We send a frame to flush our side and read should now error or the
    // socket is half-closed. We just want to verify close didn't hang.
    let _ = bot
        .writer
        .write_frame(&serialize_bot_msg(&BotMsg::PreprocessingDone))
        .await;
    let _ = bot.writer.into_inner().shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_player_recv_returns_none_on_peer_disconnect() {
    let (mut player, bot) = one_tcp_player(EventSink::noop()).await;

    drop(bot);

    let got = timeout(Duration::from_secs(2), player.recv())
        .await
        .expect("recv timeout")
        .expect("recv shouldn't error on clean close");
    assert!(got.is_none(), "expected None, got {got:?}");
}

// ── Sanity: codec wiring ──────────────────────────────────────────────

/// Quick smoke that ensures the option-defs path round-trips, since the
/// fake bot uses `vec![]` everywhere — this protects us from a silent
/// regression in OptionDef serialization.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fake_bot_can_send_options_in_identify() {
    let (listener, addr) = bound_listener().await;
    let expected = vec![(PlayerSlot::Player1, "opts".into())];
    let host = tokio::spawn(async move {
        accept_players(
            &listener,
            &expected,
            EventSink::noop(),
            Duration::from_secs(5),
        )
        .await
    });
    let mut bot = FakeBot::connect(addr).await;
    bot.send(BotMsg::Identify {
        name: "opts-bot".into(),
        author: "tests".into(),
        agent_id: "opts".into(),
        options: vec![OptionDef {
            name: "depth".into(),
            option_type: OptionType::Spin,
            default_value: "3".into(),
            min: 1,
            max: 20,
            choices: vec![],
        }],
    })
    .await;
    let _welcome = bot.recv().await;
    let [p1, _] = host.await.unwrap().expect("accept_players ok");
    assert_eq!(p1.unwrap().identity().agent_id, "opts");
}
