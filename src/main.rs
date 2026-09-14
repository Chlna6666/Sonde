use std::io;

/// Microsoft mimalloc v3 (`mimalloc` 0.1.52+ default). Do not enable the crate `v2` feature.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[actix_web::main]
async fn main() -> io::Result<()> {
    sonde::run().await
}
