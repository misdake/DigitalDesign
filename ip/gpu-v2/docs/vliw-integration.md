# VLIW 前后半合体

`system::vliw::verilog` 导出 `gpu_v2_vliw`：当前 V/T/R/I、共享结果池、Lighting、采样、Final、ROP 和 framebuffer。几何与插值数值采用 `vliw-v2@72488fd`，Vertex 源码逐字节保持，来源见 `frontend/vliw/origin.json`。这是独立 GPU IP；CPU 系统的 GPU 占位模块尚未替换，不代表 CPU、显示或板上整机资格。

## 数据流和生命周期

每个 meshlet 先完成全部 Vertex，再按每个64位 index 字中的三组三角索引依次完成 Triangle。T 的19个54位结果连续送入准备链。每个 T 的导入、光栅、插值及算术返回排空即可换下一个 T；已发布 quad 继续由后半消费，不阻止 T 局部换包。全局 uniform、纹理参数、光照、draw 和初始化表的变更，必须等待顶层 `pipeline_idle`。

R/I 直接向结果池写原始固定延迟返回。删除旧 I 的13字串行导出、旧后半八行入口收集及其重复 payload 银行；没有接一个转换器把旧运输过程藏起来。T 的19字算术结果仍是必要输入，与已删除的 I 导出无关。

结果池有32个物理槽、最多24个活动槽和8个未发布槽。分配、发布、Light、Sample 和退休是有序游标；计数带一位回绕信息，不用 freelist 或动态端口申请。8个未发布槽保证 I 的3位 quad 标签不会在固定返回结束前重用。旧 quad 的最终消费只释放其物理地址，不搬运内容。

| 内容 | 物理布局 | 写者 | 读者／最后使用 |
| --- | --- | --- | --- |
| normal | 独立512×36 BSRAM，使用32×4行 | I 的法线返回，按固定相位组合 | Light 的独立同步口 |
| UV | 独立512×36 BSRAM，每行低32位为U16/V16 | I 的属性返回 | Sample 的独立同步口 |
| Basic | 独立512×36 BSRAM；每像素两行，RGB24／Z16 | RGB在相位0，Z在相位1，静态互斥 | Final／ROP 的独立同步口 |
| NDC | 320×32只读 BSRAM，每行两个相邻坐标 | 初始化常量 | Light，同步读 |
| 头、mask、LOD | 小型分列SSRAM，按实际独立读者复制 | quad 接受、数值 mask、最终 LOD | 各有序游标；复制端口计入资源 |

NDC 为主仓 S16F14 的逐像素中心：X=`RNE((2x+1−400)×16384/400)`，Y=`RNE((240−2y−1)×16384/240)`。它不是 VLIW clip 坐标，也没有用近似增量替代舍入。

数值返回不等待后半 CE。根据正式生产者的固定返回日历，RGB 的最后属性字、UV 的最终 LOD 各推进一次完成前缀；发布只观察沿前前缀，不足空间只在接受新 quad 时背压。旧几何／属性／深度／LOD完整条件仍由非综合影子逐沿核对，详见[合体资格](vliw-qualification.md)。生产 CE 与后半 CE 独立，每条 BSRAM 是真实1W1R，无 payload 写端竞争。已经排定的返回不能被后半暂停丢弃。

## 消费和退休

Light 每像素四个有效沿：读 normal 与 X 对，捕 X 并读 Y 对，提交 normal 行，提交 NDC 行。Sample 先读 UV，保持同步 Q 至消费者接受；坐标核每四个有效沿接受一个 lane。描述符随已有同步读提前捕获，和对应 payload 一起保持，不把异步32槽读选择串入共同推进判断。

Lighting 使用完整5位物理槽。采样／Color 保留原16个公共 owner，以独立信用检查禁止相同低4位的两个 owner 同时存在；发布时保存槽号高位，Color 返回时恢复完整5位地址。这个局部信用只限制 Sample，不减少整个结果池的24个活动槽。最后一次实际 Color 写回后才能复用公共 owner。

最终数值 mask 为0的槽仍按序发布和释放，Light 与 Sample 跳过 payload。退休头只有在此前 Final／Basic／深度返回排空、发射与退休游标相等时才直接跳过：不进入着色，不给 ROP 生成八行，不产生 framebuffer 请求。这里没有 early-Z；真实深度比较由唯一 ROP 执行。

ROP 最终保存 D16 和 RGB565；前半提供的 RGB888、normal S12F10、UV16 与最终 LOD U5F3 均保持当前数值契约。前半没有 near，正向／数值有效检查与 far D16 比较继续按当前实现执行。

## 配置和外部服务

本阶段仍暴露原始64位 DMA 写口及后半128字节 byte32 事务口，作为内存接入前的明确边界。DMA 写入与计算 CE 分离；调用方必须遵守活动页和 uniform 所有权。全局初始化表只在整条 GPU 空闲时接收，普通 T 换包不等待后半退休。

纹理物理 Slot 目前仍采用主仓方形、2的幂布局。前半独立U/V尺寸字段不能自动使后半支持矩形物理纹理；当前完整资格使用一致的64×64纹理。矩形寻址不是本次回放已经验证的功能。

## 故障与背压

正常容量、同步返回及消费者暂停仍由真实 ready／信用控制，不延后一拍检查。各计算叶保留本地 preflight，非法数值或协议的当前操作在本地拒绝；跨模块诊断在物理沿锁存到顶层原有的 sticky stop，避免把多级异常判断串进共同 CE。故障发现沿的其它独立合法操作可以完成，后继沿停止计算，需要 reset 恢复；不承诺非法事务的原子回滚。外部 abort 当沿停止计算，已呈现／接受的不可撤回内存事务继续按墙钟排空。

## 验证入口

`examples/vliw_integration/verify_stage2.py` 从原始V96／uniform／index重算 V/T/R/I，并用主仓独立 Lighting 数学核对真实法线的光照，逐采样、Final 和全部 framebuffer 字节比对。box 与 rabbit 分开报告；空 quad、递归／准备气泡、真实 cache 事务和背压均计时。脚本与RTL都有上限，不将局部II4或排空突发当作整帧吞吐。

Final使用两项结果容量，以沿前队列数与算术存活数决定是否接受输入，不借当沿输出ACK；队列有旧结果时优先输出旧head，BASE只能在空队列时直接提交。这保留背压后的连续追赶能力。

当前来源、同配置拟合与有限场景结果见[最终关键证据](../examples/vliw_integration/stage4-evidence.json)；[阶段2关键证据](../examples/vliw_integration/stage2-evidence.json)保留为合体基线；生成网表、波形和历史失败试验保留在任务 `target/`，不混入实现提交。
