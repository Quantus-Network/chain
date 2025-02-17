// use wtransport::{Endpoint, ServerConfig, Identity};
// use std::error::Error;

// #[tokio::main]
// async fn main() -> Result<(), Box<dyn Error>> {
//     // Load TLS identity (replace with your cert & key files)
//     let identity = Identity::load_pemfiles("cert.pem", "key.pem").await?;

//     // Configure the server
//     let config = ServerConfig::builder()
//         .with_bind_default(4433) // Listening on port 4433
//         .with_identity(identity)
//         .build();

//     // Start the server endpoint
//     let server = Endpoint::server(config)?;

//     println!("Server is running on port 4433...");

//     loop {
//         let incoming_session = server.accept().await;  // No `?`, it's not a Result
//         let incoming_request = incoming_session.await?; // Only use `?` here
//         let connection = incoming_request.accept().await?;


//         println!("New connection accepted!");
//     }
// }



use wtransport::{Endpoint};
use std::error::Error;
use wtransport::ClientConfig;
use wtransport::Identity;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let identity = Identity::load_pemfiles("cert.pem", "key.pem").await?;
    let config = ClientConfig::builder()
        .build();

    let connection = Endpoint::client(config)?
        .connect("https://[::1]:4433")
        .await?;

    let stream = connection.open_bi().await?;
    Ok(())
}



