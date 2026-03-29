export interface Device {
  id: string;
  name: string;
  ip: string;
  online: boolean;
  lastSeen: number;
}

export interface Message {
  id: string;
  type: 'message' | 'file';
  from: string;
  fromName: string;
  to: string;
  content: string;
  timestamp: number;
  fileName?: string;
  fileSize?: number;
  progress?: number;
}

export interface NetworkMessage {
  type: 'message' | 'file' | 'heartbeat' | 'offline';
  from: string;
  fromName: string;
  to: string;
  content: string;
  timestamp: number;
  fileName?: string;
  fileSize?: number;
}
