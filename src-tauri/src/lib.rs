mod network;

use network::{NetworkState, Device};
use std::sync::{Arc, Mutex};
use tauri::State;

#[tauri::command]
fn get_device_info(state: State<'_, Arc<Mutex<NetworkState>>>) -> serde_json::Value {
    let s = state.lock().unwrap();
    serde_json::json!({
        "id": s.device_id,
        "name": s.device_name,
    })
}

#[tauri::command]
fn get_devices(state: State<'_, Arc<Mutex<NetworkState>>>) -> Vec<Device> {
    let s = state.lock().unwrap();
    let devices = s.devices.lock().unwrap();
    devices.values().cloned().collect()
}

#[tauri::command]
async fn send_message(
    to_ip: String,
    state: State<'_, Arc<Mutex<NetworkState>>>,
    content: String,
) -> Result<(), String> {
    let (from_id, from_name, to_id) = {
        let s = state.lock().unwrap();
        let devices = s.devices.lock().unwrap();
        let to_id = devices.iter()
            .find(|(_, d)| d.ip == to_ip)
            .map(|(id, _)| id.clone())
            .unwrap_or_default();
        (s.device_id.clone(), s.device_name.clone(), to_id)
    };
    
    network::send_message(to_ip, from_id, from_name, to_id, content).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let network_state = Arc::new(Mutex::new(NetworkState::new()));
    let net_state_clone = network_state.clone();

    tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = net_state_clone.clone();
            
            // 克隆 handle 供第二个任务使用
            let handle2 = handle.clone();
            
            tauri::async_runtime::spawn(async move {
                network::start_discovery(state.clone(), handle.clone()).await;
            });
            
            let state2 = net_state_clone.clone();
            tauri::async_runtime::spawn(async move {
                network::start_message_server(state2, handle2).await;
            });
            
            Ok(())
        })
        .manage(network_state)
        .invoke_handler(tauri::generate_handler![
            get_device_info,
            get_devices,
            send_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
