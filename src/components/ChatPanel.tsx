import { useState, useEffect, useRef, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { readFile } from '@tauri-apps/plugin-fs';
import { Message } from '../types';

interface Props {
  deviceId: string;
  deviceIp: string;
  deviceName: string;
}

interface FileRequest {
  id: string;
  from: string;
  fromName: string;
  fileName: string;
  fileSize: number;
  timestamp: number;
}

interface PendingFile {
  id: string;
  fileName: string;
  fileSize: number;
  fileData: number[];
  progress: number;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
  if (bytes < 1024 * 1024 * 1024) return (bytes / 1024 / 1024).toFixed(1) + ' MB';
  return (bytes / 1024 / 1024 / 1024).toFixed(1) + ' GB';
}

function ChatPanel({ deviceId, deviceIp, deviceName }: Props) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [isDragging, setIsDragging] = useState(false);
  const [pendingFiles, setPendingFiles] = useState<PendingFile[]>([]);
  const [fileRequests, setFileRequests] = useState<FileRequest[]>([]);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    // 监听收到的消息
    const unlistenMsg = listen<{from: string, fromName: string, content: string, timestamp: number, type: string}>('message-received', (event) => {
      const msg = event.payload;
      if (msg.from === deviceId) {
        setMessages(prev => [...prev, {
          id: Date.now().toString(),
          type: 'message',
          from: msg.from,
          fromName: msg.fromName,
          to: '',
          content: msg.content,
          timestamp: msg.timestamp,
        }]);
      }
    });

    // 监听文件请求
    const unlistenFileReq = listen<FileRequest>('file-request', (event) => {
      if (event.payload.from === deviceId) {
        setFileRequests(prev => [...prev, event.payload]);
      }
    });

    // 监听文件接收中
    const unlistenFileReceiving = listen<{id: string, from: string, fromName: string, fileName: string, fileSize: number}>('file-receiving', (event) => {
      if (event.payload.from === deviceId) {
        setMessages(prev => [...prev, {
          id: Date.now().toString(),
          type: 'file',
          from: event.payload.from,
          fromName: event.payload.fromName,
          to: '',
          content: `正在接收文件: ${event.payload.fileName} (${formatFileSize(event.payload.fileSize)})`,
          timestamp: event.payload.timestamp,
        }]);
      }
    });

    // 监听文件已接收
    const unlistenFileRecv = listen<{id: string, from: string, fileName: string, savePath: string}>('file-received', (event) => {
      if (event.payload.from === deviceId) {
        setMessages(prev => [...prev, {
          id: Date.now().toString(),
          type: 'file',
          from: event.payload.from,
          fromName: '系统',
          to: '',
          content: `已自动接收文件: ${event.payload.fileName}\n保存至: ${event.payload.savePath}`,
          timestamp: Date.now(),
        }]);
      }
    });

    // 监听文件被接受
    const unlistenFileAccept = listen<{id: string, from: string}>('file-accepted', (event) => {
      setPendingFiles(prev => prev.map(p => 
        p.id === event.payload.id ? {...p, progress: 100} : p
      ));
      // 移除已完成的
      setTimeout(() => {
        setPendingFiles(prev => prev.filter(p => p.id !== event.payload.id));
        setMessages(prev => [...prev, {
          id: Date.now().toString(),
          type: 'file',
          from: 'self',
          fromName: '我',
          to: deviceId,
          content: `文件已发送`,
          timestamp: Date.now(),
        }]);
      }, 1000);
    });

    // 监听文件被拒绝
    const unlistenFileReject = listen<{id: string, from: string}>('file-rejected', (event) => {
      setPendingFiles(prev => prev.filter(p => p.id !== event.payload.id));
      setMessages(prev => [...prev, {
        id: Date.now().toString(),
        type: 'file',
        from: 'system',
        fromName: '系统',
        to: '',
        content: `对方拒绝了文件传输`,
        timestamp: Date.now(),
      }]);
    });

    return () => {
      unlistenMsg.then(fn => fn());
      unlistenFileReq.then(fn => fn());
      unlistenFileReceiving.then(fn => fn());
      unlistenFileRecv.then(fn => fn());
      unlistenFileAccept.then(fn => fn());
      unlistenFileReject.then(fn => fn());
    };
  }, [deviceId]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const handleSend = async () => {
    if (!input.trim() || sending) return;
    
    setSending(true);
    try {
      await invoke('send_message', {
        toIp: deviceIp,
        content: input.trim(),
      });
      
      setMessages(prev => [...prev, {
        id: Date.now().toString(),
        type: 'message',
        from: 'self',
        fromName: '我',
        to: deviceId,
        content: input.trim(),
        timestamp: Date.now(),
      }]);
      setInput('');
    } catch (e) {
      console.error('发送失败:', e);
      alert('发送失败: ' + e);
    } finally {
      setSending(false);
    }
  };

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
  }, []);

  const handleDrop = useCallback(async (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
    
    const files = Array.from(e.dataTransfer.files);
    if (files.length === 0) return;
    
    for (const file of files) {
      const confirmed = window.confirm(`确定发送文件 "${file.name}" (${formatFileSize(file.size)}) 给 ${deviceName} 吗？`);
      if (!confirmed) continue;
      
      const arrayBuffer = await file.arrayBuffer();
      const fileData = Array.from(new Uint8Array(arrayBuffer));
      
      try {
        const fileId = await invoke<string>('send_file_request', {
          toIp: deviceIp,
          fileName: file.name,
          fileSize: file.size,
        });
        
        setPendingFiles(prev => [...prev, {
          id: fileId,
          fileName: file.name,
          fileSize: file.size,
          fileData,
          progress: 0,
        }]);
        
        // 发送文件数据
        await invoke('send_file', {
          toIp: deviceIp,
          fileId,
          fileName: file.name,
          fileData,
        });
        
        setMessages(prev => [...prev, {
          id: Date.now().toString(),
          type: 'file',
          from: 'self',
          fromName: '我',
          to: deviceId,
          content: `正在发送文件: ${file.name} (${formatFileSize(file.size)})`,
          timestamp: Date.now(),
        }]);
      } catch (e) {
        console.error('发送文件失败:', e);
        alert('发送文件失败: ' + e);
      }
    }
  }, [deviceIp, deviceName]);

  const handleFileSelect = async () => {
    try {
      const selected = await open({
        multiple: true,
        title: '选择文件',
      });
      
      if (!selected) return;
      
      const files = Array.isArray(selected) ? selected : [selected];
      
      for (const filePath of files) {
        const fileName = filePath.split(/[/\\]/).pop() || 'unknown';
        const confirmed = window.confirm(`确定发送文件 "${fileName}" 给 ${deviceName} 吗？`);
        if (!confirmed) continue;
        
        try {
          // 使用 fs 插件读取文件
          const fileContent = await readFile(filePath);
          const fileData = Array.from(fileContent);
          
          const fileId = await invoke<string>('send_file_request', {
            toIp: deviceIp,
            fileName,
            fileSize: fileContent.length,
          });
          
          setPendingFiles(prev => [...prev, {
            id: fileId,
            fileName,
            fileSize: fileContent.length,
            fileData,
            progress: 0,
          }]);
          
          await invoke('send_file', {
            toIp: deviceIp,
            fileId,
            fileName,
            fileData,
          });
          
          setMessages(prev => [...prev, {
            id: Date.now().toString(),
            type: 'file',
            from: 'self',
            fromName: '我',
            to: deviceId,
            content: `正在发送文件: ${fileName} (${formatFileSize(fileContent.length)})`,
            timestamp: Date.now(),
          }]);
        } catch (e) {
          console.error('发送文件失败:', e);
          alert('发送文件失败: ' + e);
        }
      }
    } catch (e) {
      console.error('选择文件失败:', e);
    }
  };

  const handleAcceptFile = async (request: FileRequest) => {
    try {
      await invoke('respond_file_accept', {
        toIp: deviceIp,
        fileId: request.id,
      });
      
      setFileRequests(prev => prev.filter(r => r.id !== request.id));
      setMessages(prev => [...prev, {
        id: Date.now().toString(),
        type: 'file',
        from: request.from,
        fromName: request.fromName,
        to: '',
        content: `正在接收文件: ${request.fileName} (${formatFileSize(request.fileSize)})`,
        timestamp: request.timestamp,
      }]);
    } catch (e) {
      console.error('接受文件失败:', e);
      alert('接受文件失败: ' + e);
    }
  };

  const handleRejectFile = async (request: FileRequest) => {
    try {
      await invoke('respond_file_reject', {
        toIp: deviceIp,
        fileId: request.id,
      });
      
      setFileRequests(prev => prev.filter(r => r.id !== request.id));
    } catch (e) {
      console.error('拒绝文件失败:', e);
    }
  };

  return (
    <div className="chat-panel">
      <header className="chat-header">
        <h3>{deviceName}</h3>
        <span className="device-ip">{deviceIp}</span>
        <button className="btn-file" onClick={handleFileSelect}>📎 发送文件</button>
      </header>
      
      {/* 文件请求弹窗 */}
      {fileRequests.length > 0 && (
        <div className="file-requests">
          {fileRequests.map(req => (
            <div key={req.id} className="file-request-card">
              <div className="file-request-info">
                <span className="file-name">{req.fileName}</span>
                <span className="file-size">{formatFileSize(req.fileSize)}</span>
                <span className="from-name">来自 {req.fromName}</span>
              </div>
              <div className="file-request-actions">
                <button className="btn-accept" onClick={() => handleAcceptFile(req)}>接收</button>
                <button className="btn-reject" onClick={() => handleRejectFile(req)}>拒绝</button>
              </div>
            </div>
          ))}
        </div>
      )}
      
      <div 
        className={`messages ${isDragging ? 'dragging' : ''}`}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        {isDragging && (
          <div className="drop-overlay">
            <div className="drop-text">松开以发送文件</div>
          </div>
        )}
        
        {messages.length === 0 ? (
          <div className="no-messages">发送消息开始聊天，或拖拽文件到此处发送</div>
        ) : (
          messages.map((msg) => (
            <div key={msg.id} className={`message ${msg.from === 'self' || msg.from === '我' ? 'sent' : msg.from === 'system' ? 'system' : 'received'}`}>
              <div className="message-content">{msg.content}</div>
              <div className="message-time">
                {new Date(msg.timestamp).toLocaleTimeString()}
              </div>
            </div>
          ))
        )}
        <div ref={messagesEndRef} />
      </div>
      
      <div className="input-area">
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyPress={handleKeyPress}
          placeholder="输入消息..."
          disabled={sending}
        />
        <button onClick={handleSend} disabled={sending || !input.trim()}>
          {sending ? '发送中...' : '发送'}
        </button>
      </div>
    </div>
  );
}

export default ChatPanel;
