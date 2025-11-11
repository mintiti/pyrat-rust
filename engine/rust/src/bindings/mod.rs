//! Python bindings for the `PyRat` engine
#![allow(clippy::missing_const_for_fn)] // Disable const fn warnings globally for bindings
mod game;
mod validation;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register_types_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    game::register_types(m)?;
    Ok(())
}

pub(crate) fn register_game_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    game::register_game(m)?;
    Ok(())
}

pub(crate) fn register_observation_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    game::register_observation(m)?;
    Ok(())
}

pub(crate) fn register_builder_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    game::register_builder(m)?;
    Ok(())
}
