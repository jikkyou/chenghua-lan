import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Message } from '../types';

interface Props {
  deviceId: string;
  deviceIp: string;
  deviceName: string;
}

function ChatPanel({ deviceId, deviceIp, deviceName }: Props) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // 监听收到的消息
    const unlisten = listen<{from: string, fromName: string, content: string, timestamp: number, type: string}>('message-received', (event) => {
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

    return () => {
      unlisten.then(fn => fn());
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

  return (
    <div className="chat-panel">
      <header className="chat-header">
        <h3>{deviceName}</h3>
        <span className="device-ip">{deviceIp}</span>
      </header>
      <div className="messages">
        {messages.length === 0 ? (
          <div className="no-messages">发送消息开始聊天</div>
        ) : (
          messages.map((msg) => (
            <div key={msg.id} className={`message ${msg.from === 'self' || msg.from === '我' ? 'sent' : 'received'}`}>
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
