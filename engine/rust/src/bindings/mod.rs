//! Python bindings for the `PyRat` engine
#![allow(clippy::missing_const_for_fn)] // Disable const fn warnings globally for bindings
mod game;
mod validation;

use pyo3::prelude::PyModule;
use pyo3::PyResult;

pub(crate) fn register_module(m: &PyModule) -> PyResult<()> {
    game::register_module(m)?;
    Ok(())
}
