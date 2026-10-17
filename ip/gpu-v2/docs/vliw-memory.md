# VLIW GPU 的主仓内存接入

`system::vliw_sdram::verilog` 导出 `gpu_v2_vliw_sdram`。它在现有 V/T/R/I 与完整后半之间保持同一数值契约，在输入侧加入静态 meshlet loader，并把几何／纹理只读、framebuffer refill、framebuffer writeback 接到主仓三类 GPU 内存端口。没有接入 CPU 系统的 GPU 占位模块，也没有新增大型命令处理器。

本阶段的源码身份、完整功能结果和原始收据入口统一见[阶段3关键证据](../examples/vliw_integration/stage3-evidence.json)。生成 RTL、内存种子、日志和逐字节转储保留在任务 `target/`，不随此文档入库。同源的完整三事务口GPU导出已经通过54MHz两布局，数字统一见关键证据；该拟合包含装填器与端口控制，不包含外部SDRAM控制器、CPU或显示。

## 端口与地址单位

| 边界 | 地址与长度 | 事务及返回 |
| --- | --- | --- |
| host descriptor | config／metadata／payload 三个22位半字源地址；config／payload 字节长度；page 与 tag | 一次接受一个作业；metadata 固定32字节，payload 最大1KiB |
| 几何和纹理共享 RO | 主仓22位半字地址，32／64／128字节自然对齐；长度码为0／1／3 | 事务级轮转选择，保存 owner 到实际 last／error，不按数据拍切换 |
| framebuffer read | 后半 byte32 的128字节扇区，经范围／对齐检查后转半字22位 | 独立主仓 GPU read 端口 |
| framebuffer write | 同上，64位连续写数据直接来自 cache | 独立主仓 GPU write 端口，不新增整包FIFO |

物理容量为8MiB。descriptor 的非空字段必须是8字节整数倍；metadata／config 源至少32字节对齐，payload 起点128字节对齐。末尾不足32字节时，物理事务仍读完整32字节，只有有效部分写本地页。loader 按剩余长度及自然边界选择突发；不能用byte地址直接连主仓半字端口。

原后半纹理／framebuffer 二次仲裁在新工厂的导出结果中整体移除。纹理事务与 framebuffer 事务独立保留，RO侧只对几何和纹理仲裁。响应没有 ready，返回、DMA 写与已接受事务的完成均不受计算 CE 冻结。

## 固定目的地址与页寿命

外部不能任意指定 DMA 目的地址。wrapper 固定使用64位字地址：payload 为 `page*128`，metadata 为 `256+page*4`，config 从320开始，长度不超过512字节。输入页使用独立 DMA 写口，不借计算读口。

loader 只在目标页当前可写时接受作业，并在整笔作业期间保持该页寿命。活动页由真实 `start` 接受沿捕获；其它页可与当前计算重叠预取。前半已排空后，输入页可以复用，无需等待已经复制到结果池的旧 quad 退休。

接受 descriptor 时先使目标页失效；全部有效字段写完且事务成功后，`completion_valid && completion_publish` 才使页面可执行。成功 completion 即使尚未由 host ACK，也允许启动已经稳定的页面。失败刷新不能启动；config 失败会使两页失效，避免把部分全局写当作完整配置。host 应显式提交 source、page 和 start，wrapper 不推测软件 mesh 遍历。

全局 config 接受要求完整 GPU、内存事务和前一作业排空，包含已经接受的整个 flush 流程及两次 writeback 之间的空隙。config descriptor 等待期间禁止新的冲突 start；同沿 descriptor／同页 start 由 descriptor 优先保护。初始化表也遵守完整 drain。

## 背压、abort 与故障

一旦请求已呈现，即使 ready 尚未到达，地址和长度也必须保持到接受；abort 不撤回已呈现请求。已经接受的事务继续收至实际 last／error。错误只完成其锁定 owner，不能错投其它 client；错误不保证此前本地写或 framebuffer 写回滚。reset 必须同时复位实际传输链，局部取消使用 abort 并排空。

任一 fault 都阻止新 meshlet 启动；外部 abort 与计算叶故障按计算 fail-stop 处理。router fault 会取消 loader 后继工作，并要求调用方复位恢复；当前接线不承诺其它已有计算在同沿原子停止。已经呈现的 descriptor 仍须按付费事务保持并排空，不能据此声称故障瞬间禁止所有新物理请求。Final 的两项结果容量按旧 `qcount+arith_live<2` 接受输入，不借当沿输出 ACK；队列非空先消费旧 head，BASE 结果只在旧队列为空时直通。

## 复现与证据边界

```powershell
cargo test -p gpu-v2 --test vliw_dma -- --include-ignored --nocapture
cargo test -p gpu-v2 --test vliw_memory -- --include-ignored --nocapture
python ip/gpu-v2/tests/vliw_sdram.py --tag replay-stage3-new --mode both --stress --native-rabbit
```

每次完整回放必须使用新 tag。环境使用 `IVERILOG_EXE`、`VVP_EXE`、`GOWIN_HOME`；并行 Cargo 作业应使用独立 `CARGO_TARGET_DIR`。完整 runner 有有限周期上限和进程超时，保存实际硬件依赖清单并检查运行中源码未变。

portable 运行真实兔子与 box，native 运行两页完整兔子；两份硬件源清单相同，但负载不同，不要求跨负载 trace 相等。它们从真实 SDRAM 引脚装填输入，逐项核对 V/T/raw19/h/I、Lighting、采样、Final 和物理 framebuffer。portable 定向覆盖脏 flush 阻挡 config；重复兔子的 native 用例覆盖真实 LESS 拒绝，末尾缓存可能已干净。

引脚仿真的相关时钟约为49.5MHz逻辑／99MHz控制器，属于功能模型证据。它不代替54MHz布局布线、CPU／显示共同争用、任意纹理工作集或板测。malformed／abort 的详细故障注入属于冻结 loader／router 单体收据；不能把正常整链回放扩大为全部故障资格。
