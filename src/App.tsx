import { useState } from 'react';
import DeviceList from './components/DeviceList';
import ChatPanel from './components/ChatPanel';
import './App.css';

function App() {
  const [selectedDevice, setSelectedDevice] = useState<{
    id: string;
    ip: string;
    name: string;
  } | null>(null);

  const handleSelectDevice = (id: string, ip: string, name: string) => {
    setSelectedDevice({ id, ip, name });
  };

  return (
    <div className="app">
      <aside className="sidebar">
        <h2>成华县过县</h2>
        <DeviceList onSelectDevice={handleSelectDevice} selectedDevice={selectedDevice?.id || null} />
      </aside>
      <main className="main-content">
        {selectedDevice ? (
          <ChatPanel
            deviceId={selectedDevice.id}
            deviceIp={selectedDevice.ip}
            deviceName={selectedDevice.name}
          />
        ) : (
          <div className="placeholder">
            <p>选择一个设备开始聊天</p>
          </div>
        )}
      </main>
    </div>
  );
}

export default App;
