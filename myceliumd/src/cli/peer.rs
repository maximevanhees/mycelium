use mycelium::peer_manager::PeerStats;
use mycelium_api::AddPeer;
use prettytable::{row, Table};
use std::net::SocketAddr;
use tracing::{debug, error};

/// List the peers the current node is connected to
pub async fn list_peers(server_addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    // Make API call
    let request_url = format!("http://{server_addr}/api/v1/admin/peers");
    match reqwest::get(&request_url).await {
        Err(e) => {
            error!("Failed to retrieve peers");
            return Err(e.into());
        }
        Ok(resp) => {
            debug!("Listing connected peers");
            match resp.json::<Vec<PeerStats>>().await {
                Err(e) => {
                    error!("Failed to load response json: {e}");
                    return Err(e.into());
                }
                Ok(peers) => {
                    let mut table = Table::new();
                    table.add_row(row![
                        "Protocol",
                        "Socket",
                        "Type",
                        "Connected",
                        "Rx total",
                        "Tx total"
                    ]);
                    for peer in peers.iter() {
                        table.add_row(row![
                            peer.endpoint.proto(),
                            peer.endpoint.address(),
                            peer.pt,
                            peer.connection_state,
                            peer.rx_bytes,
                            peer.tx_bytes
                        ]);
                    }
                    table.printstd();
                }
            }
        }
    };

    Ok(())
}

/// Remove peer(s) by (underlay) IP
pub async fn remove_peers(
    server_addr: SocketAddr,
    peers: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    for peer in peers.iter() {
        // encode to pass in URL
        let peer_encoded = urlencoding::encode(peer);
        let request_url = format!("http://{server_addr}/api/v1/admin/peers/{peer_encoded}");
        if let Err(e) = client
            .delete(&request_url)
            .send()
            .await
            .and_then(|res| res.error_for_status())
        {
            error!("Failed to delete peer: {e}");
            return Err(e.into());
        }
    }

    Ok(())
}

/// Add peer(s) by (underlay) IP
pub async fn add_peers(
    server_addr: SocketAddr,
    peers: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    for peer in peers.into_iter() {
        let request_url = format!("http://{server_addr}/api/v1/admin/peers");
        if let Err(e) = client
            .post(&request_url)
            .json(&AddPeer { endpoint: peer })
            .send()
            .await
            .and_then(|res| res.error_for_status())
        {
            error!("Failed to add peer: {e}");
            return Err(e.into());
        }
    }

    Ok(())
}
