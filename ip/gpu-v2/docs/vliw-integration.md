# VLIW 前后半合体

`system::vliw::verilog` 导出 `gpu_v2_vliw`：当前 V/T/R/I、共享结果池、Lighting、采样、Final、ROP 和 framebuffer。几何与插值起源为 `vliw-v2@72488fd`，历史来源见 `frontend/vliw/origin.json`；当前 Vertex 乘加、位置计算和控制 ROM 保持，法线量化已提前至输出打包，当前源码身份见[布局关键证据](../examples/vliw_integration/layout-evidence.json)。这是独立 GPU IP；CPU 系统的 GPU 占位模块尚未替换，不代表 CPU、显示或板上整机资格。

## 数据流和生命周期

每个 meshlet 先完成全部 Vertex，再按每个64位 index 字中的三组三角索引依次完成 Triangle。T 的16个54位运输项连续送入准备链。每个 T 的导入、光栅、插值及算术返回排空即可换下一个 T；已发布 quad 继续由后半消费，不阻止 T 局部换包。全局 uniform、纹理参数、光照、draw 和初始化表的变更，必须等待顶层 `pipeline_idle`。

R/I 直接向结果池写原始固定延迟返回。删除旧 I 的13字串行导出、旧后半八行入口收集及其重复 payload 银行；没有接一个转换器把旧运输过程藏起来。

### V 结果、T 导出与准备链

| 数据 | 当前格式与地址 | 生产、存储和消费 |
| --- | --- | --- |
| V 的 P.xyz | 各32位，位置银行 plane0..2 | 原位置计算与写槽不变；T 独立同步口读取 |
| V 的 N.xyz | 三个 S12F10 拼成 N36，导出银行 plane3 | 原 Nx/Ny/Nz pack 槽顺序共用一个 RNE／饱和器；Nz 完成后一次写入 |
| V 的属性 | UV16×2 或 RGB888，32位，导出银行 plane4 | 原属性产生槽；不再访问旧 plane5 |
| T 的前10项 | 三组几何系数和 det；运输容器54位 | 低36位原银行、高18位新增独立 BSRAM；沿用原低／高写槽，同 slot/item 地址并行读 |
| T 的后6项 | 顶点0、1、2各 N36、属性32 | 每项一次读；高18位静态补零，不读取高字银行 |

法线量化迁移保持位精确：normal18 先保留原低18位回绕；normal16 先保留原 `pack_value[17:2]` 的有符号16位截位，再按旧准备阶段恢复四倍，最后使用原 RNE 和 S12F10 饱和。没有直接对更早的完整 MAC 结果舍入。旧 Nxy 的 emit 槽仍在算术 ROM 中，但不再写 RAM、发布输出或等待 `output_ready`；最终 N36 与属性分别构成 V 的第4、5项。

| 有效沿／状态 | 读取和消费 | 保持与释放 |
| --- | --- | --- |
| T 导出发读 | 前10项同时读低36／高18银行；后6项读属性银行 | 两银行各自一沿同步 Q，没有高字标签和第二次读 |
| 下一有效沿 | 旧 Q 形成54位输出，同时可发后继读 | 输出被 CE／背压暂停时保持，末项15被接受后释放 T 槽 |
| Prepare TAIL | Write N36/UV32/RGB24 as three padded 16-bit PC lanes in one enabled edge | Accept the word on that write; normal rows 12..14, attribute rows 8..10 |
| ROM op0/1/2 | Read vertices 2/0/1; capture a2, then d0=a0-a2 | Reuse one signed17 subtractor and short operands; no source-word hold |
| ROM op3 or op42 | Form d1=a1-a2 and publish the final parameter row | U/V write PA in the same fixed return slots; CE freezes all owners |

Each vertex imports one normal row and one attribute row. Six final parameter rows per triangle store three exact signed17 fields: d0[16:0], d1[33:17], base[50:34]. UV differences in -65535..65535 fit exactly; DSP input wiring restores redundant sign bits and retains the half-LSB bias. SECOND/THIRD expansion is removed. 旧 `fetch_low`、`import_hi`、`Q_HI`、准备阶段 normal18 转换／RNE／模式寄存与 Nxy/Nz 分支已删除。冻结数学参考继续计算原语义，测试的 `transport.py` 只将其打包为 V5／T16；活动 RTL 没有扩回旧19项。

### R/I 到后半

结果池有32个物理槽、最多24个活动槽和8个未发布槽。分配、发布、Light、Sample 和退休是有序游标；计数带一位回绕信息，不用 freelist 或动态端口申请。8个未发布槽保证 I 的3位 quad 标签不会在固定返回结束前重用。旧 quad 的最终消费只释放其物理地址，不搬运内容。

| 内容 | 物理布局 | 写者 | 读者／最后使用 |
| --- | --- | --- | --- |
| normal | 独立512×36 BSRAM，使用32×4行 | φ2写低18、φ3写高18；仅Ny接缝高6位跨拍保持 | Light 的独立同步口 |
| UV | 独立512×36 BSRAM，`{pad2,V16,pad2,U16}` | φ3写低18、φ0写高18；删除U16暂存 | Sample 的独立同步口 |
| Basic颜色 | 独立512×36 BSRAM，每像素一行，R9/G9/B9三列 | φ3写R、φ0写G/B；每列低8位有效 | Final 的独立同步口 |
| Z | 独立18位1W1R BSRAM，使用32×4行Z16 | I原φ1返回直接写，不与颜色共端口 | sink实际接受C时读，下一配对D直接消费Q |
| NDC | 320×32只读 BSRAM，每行两个相邻坐标 | 初始化常量 | Light，同步读 |
| 头、mask、LOD | 小型分列SSRAM，按实际独立读者复制 | quad 接受、数值 mask、最终 LOD | 各有序游标；复制端口计入资源 |

NDC 为主仓 S16F14 的逐像素中心：X=`RNE((2x+1−400)×16384/400)`，Y=`RNE((240−2y−1)×16384/240)`。它不是 VLIW clip 坐标，也没有用近似增量替代舍入。

数值返回不等待后半 CE。根据正式生产者的固定返回日历，RGB 的最后属性字、UV 的最终 LOD 各推进一次完成前缀；发布只观察沿前前缀，不足空间只在接受新 quad 时背压。旧几何／属性／深度／LOD完整条件仍由非综合影子逐沿核对，详见[合体资格](vliw-qualification.md)。生产 CE 与后半 CE 独立，每条 BSRAM 是真实1W1R，无 payload 写端竞争。已经排定的返回不能被后半暂停丢弃。

Normal／UV／RGB各列的DI为固定接线，只有9位列写使能选择实际写入沿，没有再用相位选择整条36位写数据。正式属性顺序固定φ2/φ3/φ0，因此字号直接由相位表示。φ1深度返回与随后三属性字属于同一像素，写Z直接使用已有属性像素游标；独立字号与深度游标只保留为非综合影子核对。

## 消费和退休

Light 每像素四个有效沿：读 normal 与 X 对，捕 X 并读 Y 对，提交 normal 行，提交 NDC 行。Sample 先读 UV，保持同步 Q 至消费者接受；坐标核每四个有效沿接受一个 lane。描述符随已有同步读提前捕获，和对应 payload 一起保持，不把异步32槽读选择串入共同推进判断。

Lighting 使用完整5位物理槽。采样／Color 保留原16个公共 owner，以独立信用检查禁止相同低4位的两个 owner 同时存在；发布时保存槽号高位，Color 返回时恢复完整5位地址。这个局部信用只限制 Sample，不减少整个结果池的24个活动槽。最后一次实际 Color 写回后才能复用公共 owner。

最终数值 mask 为0的槽仍按序发布和释放，Light 与 Sample 跳过 payload。退休头只有在此前 Final排空、源Q不被持有、发射与退休游标相等时才直接跳过：不进入着色，不给 ROP 生成八行，不产生 framebuffer 请求。非空槽中不覆盖的lane直接跨过Final取源，不读RGB/GH/texture，也不进入源等待。这里没有 early-Z；真实深度比较由唯一 ROP 执行。

Final的RGB/GH/texture读口同为一沿同步读：读沿进入J_WAIT时三份Q已经就绪，状态本身表示所有权，CE0或Final背压期间状态和Q一起保持。删除旧paid及三条返回valid流水。发射游标经过的前缀保证退休不越过源处理，真实跨阶段容量与背压仍保留。

sink是C/D配对相位和lane的唯一权威，父层不再复制color_paid／out_lane寄存器或再比较lane身份。C真正被cache接受时，用该slot/lane读取独立Z银行；D消费旧Z Q，停顿时不覆盖。最后lane的D接受后按原边界回收quad。删除旧四行Z暂存、深度读预约及标签，发布区间直接由已有publish尾与退休／发射游标比较，不再复制父层发布计数。24个活动槽小于32，游标相等不存在满／空混淆。

ROP 最终保存 D16 和 RGB565；前半提供的 RGB888、normal S12F10、UV16 与最终 LOD U5F3 均保持当前数值契约。前半没有 near，正向／数值有效检查与 far D16 比较继续按当前实现执行。

## 配置和外部服务

本阶段仍暴露原始64位 DMA 写口及后半128字节 byte32 事务口，作为内存接入前的明确边界。DMA 写入与计算 CE 分离；调用方必须遵守活动页和 uniform 所有权。全局初始化表只在整条 GPU 空闲时接收，普通 T 换包不等待后半退休。

纹理物理 Slot 目前仍采用主仓方形、2的幂布局。前半独立U/V尺寸字段不能自动使后半支持矩形物理纹理；当前完整资格使用一致的64×64纹理。矩形寻址不是本次回放已经验证的功能。

## 故障与背压

正常容量、同步返回及消费者暂停仍由真实 ready／信用控制，不延后一拍检查。各计算叶保留本地 preflight，非法数值或协议的当前操作在本地拒绝；跨模块诊断在物理沿锁存到顶层原有的 sticky stop，避免把多级异常判断串进共同 CE。故障发现沿的其它独立合法操作可以完成，后继沿停止计算，需要 reset 恢复；不承诺非法事务的原子回滚。外部 abort 当沿停止计算，已呈现／接受的不可撤回内存事务继续按墙钟排空。

## 验证入口

`examples/vliw_integration/verify_stage2.py` 从原始V96／uniform／index重算 V/T/R/I，并用主仓独立 Lighting 数学核对真实法线的光照，逐采样、Final 和全部 framebuffer 字节比对。box 与 rabbit 分开报告；空 quad、递归／准备气泡、真实 cache 事务和背压均计时。脚本与RTL都有上限，不将局部II4或排空突发当作整帧吞吐。

Final使用两项结果容量，以沿前队列数与算术存活数决定是否接受输入，不借当沿输出ACK；队列有旧结果时优先输出旧head，BASE只能在空队列时直接提交。这保留背压后的连续追赶能力。

当前来源、同配置拟合与有限场景结果见[布局关键证据](../examples/vliw_integration/layout-evidence.json)；[Color 关键证据](../examples/vliw_integration/color-direct-evidence.json)保留前序基线。当前采用精确N12／T导出及分列结果池／独立Z银行。后续normal生产链完整改写、SSRAM高字银行和向下截断均完成受控比较，当前组合仍保留已有精确N12实现；不把整链映射变化解释为路线无效。生成网表、波形和历史失败试验不入库。
