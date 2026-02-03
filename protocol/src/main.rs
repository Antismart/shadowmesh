use shadowmesh_protocol::ShadowNode;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting ShadowMesh node...");

    let mut node = ShadowNode::new().await?;
    println!("Node created with peer ID: {:?}", node.peer_id());

    node.start().await?;
    println!("Node started and listening for connections");

    // Keep running
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
