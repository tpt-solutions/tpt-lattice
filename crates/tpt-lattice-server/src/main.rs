fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let addr = std::env::var("TPT_LATTICE_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
        eprintln!("TPT Lattice sync server listening on ws://{addr}/ws");
        tpt_lattice_server::serve(&addr).await
    })
}
