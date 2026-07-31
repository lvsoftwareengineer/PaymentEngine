pub use anyhow::Ok;
pub mod model;
pub mod store;
pub mod engine;
pub mod io;


pub fn run() -> Result<(), anyhow::Error>{
    Ok(())
}