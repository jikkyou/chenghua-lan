mod network;

use network::{NetworkState, Device};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

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

#[tauri::command]
async fn send_file_request(
    to_ip: String,
    file_name: String,
    file_size: u64,
    state: State<'_, Arc<Mutex<NetworkState>>>,
) -> Result<String, String> {
    let (from_id, from_name, to_id) = {
        let s = state.lock().unwrap();
        let devices = s.devices.lock().unwrap();
        let to_id = devices.iter()
            .find(|(_, d)| d.ip == to_ip)
            .map(|(id, _)| id.clone())
            .unwrap_or_default();
        (s.device_id.clone(), s.device_name.clone(), to_id)
    };
    
    network::send_file_request(to_ip, file_name, file_size, from_id, from_name, to_id).await
}

#[tauri::command]
async fn send_file(
    to_ip: String,
    file_id: String,
    file_name: String,
    file_data: Vec<u8>,
    state: State<'_, Arc<Mutex<NetworkState>>>,
) -> Result<(), String> {
    let from_id = state.lock().unwrap().device_id.clone();
    network::send_file(to_ip, file_id, from_id, file_name, file_data).await
}

#[tauri::command]
async fn respond_file_accept(
    to_ip: String,
    file_id: String,
    state: State<'_, Arc<Mutex<NetworkState>>>,
) -> Result<(), String> {
    let (from_id, to_id) = {
        let s = state.lock().unwrap();
        let devices = s.devices.lock().unwrap();
        let to_id = devices.iter()
            .find(|(_, d)| d.ip == to_ip)
            .map(|(id, _)| id.clone())
            .unwrap_or_default();
        (s.device_id.clone(), to_id)
    };
    
    network::respond_file_accept(to_ip, file_id, from_id, to_id).await
}

#[tauri::command]
async fn respond_file_reject(
    to_ip: String,
    file_id: String,
    state: State<'_, Arc<Mutex<NetworkState>>>,
) -> Result<(), String> {
    let (from_id, to_id) = {
        let s = state.lock().unwrap();
        let devices = s.devices.lock().unwrap();
        let to_id = devices.iter()
            .find(|(_, d)| d.ip == to_ip)
            .map(|(id, _)| id.clone())
            .unwrap_or_default();
        (s.device_id.clone(), to_id)
    };
    
    network::respond_file_reject(to_ip, file_id, from_id, to_id).await
}

#[tauri::command]
fn set_auto_receive(state: State<'_, Arc<Mutex<NetworkState>>>, enabled: bool) {
    *state.lock().unwrap().auto_receive.lock().unwrap() = enabled;
}

#[tauri::command]
fn get_auto_receive(state: State<'_, Arc<Mutex<NetworkState>>>) -> bool {
    state.lock().unwrap().auto_receive.lock().unwrap().clone()
}

#[tauri::command]
fn set_receive_folder(state: State<'_, Arc<Mutex<NetworkState>>>, folder: String) {
    *state.lock().unwrap().receive_folder.lock().unwrap() = folder;
}

#[tauri::command]
fn get_receive_folder(state: State<'_, Arc<Mutex<NetworkState>>>) -> String {
    state.lock().unwrap().receive_folder.lock().unwrap().clone()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let network_state = Arc::new(Mutex::new(NetworkState::new()));
    let net_state_clone = network_state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = net_state_clone.clone();
            
            let handle2 = handle.clone();
            
            tauri::async_runtime::spawn(async move {
                network::start_discovery(state.clone(), handle.clone()).await;
            });
            
            let state2 = net_state_clone.clone();
            tauri::async_runtime::spawn(async move {
                network::start_message_server(state2, handle2).await;
            });
            
            let state3 = net_state_clone.clone();
            let handle3 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                network::start_file_server(state3, handle3).await;
            });
            
            Ok(())
        })
        .manage(network_state)
        .invoke_handler(tauri::generate_handler![
            get_device_info,
            get_devices,
            send_message,
            send_file_request,
            send_file,
            respond_file_accept,
            respond_file_reject,
            set_auto_receive,
            get_auto_receive,
            set_receive_folder,
            get_receive_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
