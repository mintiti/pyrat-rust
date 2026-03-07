// Stub Tauri's IPC bridge so imports that call invoke() at module level
// don't blow up in Node (no window.__TAURI_INTERNALS__ outside the webview).

// biome-ignore lint/suspicious/noExplicitAny: globalThis typing escape hatch for test stub
(globalThis as any).window = {
	__TAURI_INTERNALS__: {
		invoke: async () => [],
		transformCallback: () => 0,
	},
};
