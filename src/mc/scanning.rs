use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::thread;

// 完全移除重复的logging宏定义（因为main.rs已有）
// 移除所有无用导入
pub struct MinecraftScanner {
    port: Arc<Mutex<Vec<u16>>>,
    _holder: Sender<()>,
}

impl MinecraftScanner {
    // 🔴 修复：改为无参create方法（匹配调用方的create()）
    pub fn create() -> MinecraftScanner {
        let (tx, rx) = mpsc::channel::<()>();
        let port = Arc::new(Mutex::new(vec![25565])); // 固定端口25565

        let port_cloned = Arc::clone(&port);
        thread::spawn(move || {
            let _ = Self::run(rx, port_cloned);
        });

        MinecraftScanner { _holder: tx, port }
    }

    // 仅保留退出信号和端口维护，移除filter参数
    fn run(signal: Receiver<()>, output: Arc<Mutex<Vec<u16>>>) -> Result<(), ()> {
        // 固定端口为25565，模拟活跃状态
        let mut ports: Vec<(u16, SystemTime)> = vec![(25565, SystemTime::now())];
        
        loop {
            // 检查退出信号
            if let Err(mpsc::TryRecvError::Disconnected) = signal.try_recv() {
                return Ok(());
            }

            let now = SystemTime::now();
            let mut dirty = false;
            // 遍历检查端口时效性（反向遍历避免索引错乱）
            for i in (0..ports.len()).rev() {
                if let Ok(dur) = now.duration_since(ports[i].1) {
                    if dur.as_millis() >= 5000 {
                        ports.remove(i);
                        dirty = true;
                        // 重新添加25565，保证始终存在
                        ports.push((25565, SystemTime::now()));
                        dirty = true;
                    }
                }
            }

            // 🔴 修复：dirty赋值后会被读取，更新输出列表
            if dirty {
                let mut output_lock = output.lock().unwrap();
                output_lock.clear();
                output_lock.push(25565);
                
                // 使用main.rs中已定义的logging宏
                logging!("Server Scanner", "Updating server list to [25565]");
            }

            // 降低循环频率，减少CPU占用
            thread::sleep(Duration::from_millis(200));
        }
    }

    // 保持原接口不变
    pub fn get_ports(&self) -> Vec<u16> {
        self.port.lock().unwrap().clone()
    }
}
