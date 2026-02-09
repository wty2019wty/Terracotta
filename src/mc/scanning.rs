use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::thread;

// 移除所有socket2、net相关的无用导入
// use socket2::{Domain, SockAddr, Socket, Type};
// use std::borrow::Cow;
// use std::io::Result;
// use std::mem::MaybeUninit;
// use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
// use std::str::FromStr;
// use std::mem;

pub struct MinecraftScanner {
    port: Arc<Mutex<Vec<u16>>>,
    _holder: Sender<()>,
}

impl MinecraftScanner {
    // 恢复create方法的filter参数（可选，若调用方还传参则兼容；若不需要可直接移除）
    // 如果你已经修改调用方为无参，可将参数改为 fn create() -> Self
    pub fn create(_filter: fn(&str) -> bool) -> MinecraftScanner {
        let (tx, rx) = mpsc::channel::<()>();
        let port = Arc::new(Mutex::new(vec![25565])); // 直接初始化端口为25565

        let port_cloned = Arc::clone(&port);
        thread::spawn(move || {
            // 调用简化后的run方法，仅维护端口和退出逻辑
            let _ = Self::run(rx, port_cloned);
        });

        MinecraftScanner { _holder: tx, port }
    }

    // 移除filter参数，仅保留退出信号和端口维护
    fn run(signal: Receiver<()>, output: Arc<Mutex<Vec<u16>>>) -> Result<(), ()> {
        // 固定端口为25565，模拟原逻辑的"活跃端口"（5秒有效期）
        let mut ports: Vec<(u16, SystemTime)> = vec![(25565, SystemTime::now())];
        
        loop {
            // 检查退出信号，兼容原优雅退出逻辑
            if let Err(mpsc::TryRecvError::Disconnected) = signal.try_recv() {
                return Ok(());
            }

            let now = SystemTime::now();
            // 模拟原逻辑：如果端口过期则移除（但这里25565永远不过期）
            let mut dirty = false;
            for i in (0..ports.len()).rev() {
                if matches!(now.duration_since(ports[i].1), Ok(dur) if dur.as_millis() >= 5000) {
                    ports.remove(i);
                    dirty = true;
                    // 端口过期后重新添加，保证25565始终存在
                    ports.push((25565, SystemTime::now()));
                    dirty = true;
                }
            }

            // 更新输出端口列表（始终是25565）
            if dirty {
                let mut output = output.lock().unwrap();
                output.clear();
                output.push(25565);
                
                logging!("Server Scanner", "Updating server list to [25565]");
            }

            // 降低循环频率，减少CPU占用
            thread::sleep(Duration::from_millis(200));
        }
    }

    // 保持原接口不变，调用方无需修改
    pub fn get_ports(&self) -> Vec<u16> {
        self.port.lock().unwrap().clone()
    }
}

// 兼容原代码中的logging宏（如果没有定义，可临时添加空实现）
#[macro_export]
macro_rules! logging {
    ($tag:expr, $fmt:expr, $($arg:tt)*) => {
        println!("[{}] {}", $tag, format!($fmt, $($arg)*));
    };
}
