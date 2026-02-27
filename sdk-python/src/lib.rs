use pyo3::prelude::*;

mod maze;

/// Native extension for the PyRat Python SDK.
#[pymodule]
fn _engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<maze::PyMaze>()?;
    Ok(())
}
