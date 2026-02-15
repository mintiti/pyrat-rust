use pyo3::prelude::*;

/// Python module for PyRat game engine core implementation
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let types_module = PyModule::new(m.py(), "types")?;
    pyrat::bindings::register_types_module(&types_module)?;
    m.add_submodule(&types_module)?;

    let game_module = PyModule::new(m.py(), "game")?;
    pyrat::bindings::register_game_module(&game_module)?;
    m.add_submodule(&game_module)?;

    let observation_module = PyModule::new(m.py(), "observation")?;
    pyrat::bindings::register_observation_module(&observation_module)?;
    m.add_submodule(&observation_module)?;

    let builder_module = PyModule::new(m.py(), "builder")?;
    pyrat::bindings::register_builder_module(&builder_module)?;
    m.add_submodule(&builder_module)?;

    Ok(())
}
