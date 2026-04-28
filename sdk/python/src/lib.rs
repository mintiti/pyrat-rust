use pyo3::prelude::*;

mod codec;
mod maze;
mod sim;

/// Native extension for the PyRat Python SDK.
#[pymodule]
fn _engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<maze::PyMaze>()?;
    m.add_class::<sim::PyGameSim>()?;
    m.add_class::<sim::PyMoveUndo>()?;
    m.add_function(wrap_pyfunction!(codec::parse_host_frame, m)?)?;
    m.add_function(wrap_pyfunction!(codec::parse_bot_frame, m)?)?;
    m.add_function(wrap_pyfunction!(codec::py_serialize_bot_msg, m)?)?;
    m.add_function(wrap_pyfunction!(codec::py_serialize_host_msg, m)?)?;
    Ok(())
}
