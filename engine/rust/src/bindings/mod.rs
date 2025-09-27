//! Python bindings for the `PyRat` engine
#![allow(clippy::missing_const_for_fn)] // Disable const fn warnings globally for bindings
mod game;
mod validation;

use pyo3::prelude::PyModule;
use pyo3::PyResult;

pub(crate) fn register_types_module(m: &PyModule) -> PyResult<()> {
    game::register_types(m)?;
    Ok(())
}

pub(crate) fn register_game_module(m: &PyModule) -> PyResult<()> {
    game::register_game(m)?;
    Ok(())
}

pub(crate) fn register_observation_module(m: &PyModule) -> PyResult<()> {
    game::register_observation(m)?;
    Ok(())
}

pub(crate) fn register_builder_module(m: &PyModule) -> PyResult<()> {
    game::register_builder(m)?;
    Ok(())
}
