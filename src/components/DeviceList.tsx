import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Device } from '../types';

interface Props {
  onSelectDevice: (id: string, ip: string, name: string) => void;
  selectedDevice: string | null;
}

function DeviceList({ onSelectDevice, selectedDevice }: Props) {
  const [devices, setDevices] = useState<Device[]>([]);
  const [localDevice, setLocalDevice] = useState<{id: string, name: string} | null>(null);

  useEffect(() => {
    // 获取本机设备信息
    invoke<{id: string, name: string}>('get_device_info').then(info => {
      setLocalDevice(info);
    });

    // 获取初始设备列表
    invoke<Device[]>('get_devices').then(list => {
      setDevices(list);
    });

    // 监听设备变化
    const unlisten = listen<Device[]>('devices-changed', (event) => {
      setDevices(event.payload);
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  const filteredDevices = devices.filter(d => d.online);

  return (
    <div className="device-list-container">
      {localDevice && (
        <div className="local-device">
          <span className="local-badge">本机</span>
          <span className="device-name">{localDevice.name}</span>
        </div>
      )}
      <div className="device-list-header">在线设备 ({filteredDevices.length})</div>
      {filteredDevices.length === 0 ? (
        <div className="device-list-empty">
          <p>暂未发现在线设备</p>
          <p className="hint">确保其他设备也在运行此应用</p>
        </div>
      ) : (
        <ul className="device-list">
          {filteredDevices.map((device) => (
            <li
              key={device.id}
              className={`device-item ${selectedDevice === device.id ? 'selected' : ''}`}
              onClick={() => onSelectDevice(device.id, device.ip, device.name)}
            >
              <span className="device-name">{device.name}</span>
              <span className="device-ip">{device.ip}</span>
              <span className={`status ${device.online ? 'online' : 'offline'}`} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default DeviceList;
