// src/main.rs
mod modbus_client;
use crate::modbus_client::run_modbus_client;
use anyhow::Result;
use futures::future::try_join_all;

#[tokio::main]
async fn main() -> Result<()> {
    // 定义要连接的设备列表 (地址, Slave ID)
    let devices = vec![
        ("127.0.0.1:502".parse().unwrap(), 1, 1 as u64),
        // ("127.0.0.1:503".parse().unwrap(), 1, 2 as u64),
        // ("127.0.0.1:505".parse().unwrap(), 1, 1 as u64),
        // ("127.0.0.1:506".parse().unwrap(), 1, 2 as u64),
        // 可以添加更多设备
    ];
    // 为每个设备创建一个异步任务
    let tasks: Vec<_> = devices
        .into_iter()
        .map(|(addr, slave_id, read_interval_secs)| tokio::spawn(run_modbus_client(addr, slave_id, read_interval_secs)))
        .collect();

    // 等待所有任务完成
    match try_join_all(tasks).await {
        Ok(_) => println!("所有连接已正常结束"),
        Err(e) => eprintln!("有连接异常结束: {:?}", e),
    }
    // // 创建 tokio 运行时并执行异步函数
    // let rt = tokio::runtime::Runtime::new()?;
    // // 设置 Modbus 服务器地址和端口
    // let socket_addr = "127.0.0.1:502".parse().unwrap();
    // rt.block_on(run_modbus_client(socket_addr, 1))?;
    Ok(())
}
