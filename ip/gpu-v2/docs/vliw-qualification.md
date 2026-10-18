# VLIW GPU 合体资格与资源判断

当前独立 GPU 由 `system::vliw_sdram::verilog` 导出，[布局关键证据](../examples/vliw_integration/layout-evidence.json)绑定当前精确 N12／T16 源码、P0 拟合与当前内存链完整场景比较。[Color 收据](../examples/vliw_integration/color-direct-evidence.json)是本轮直接基线，[准备收据](../examples/vliw_integration/frontend-preparation-evidence.json)、[TDP 收据](../examples/vliw_integration/framebuffer-tdp-evidence.json)和[帧缓冲合同](framebuffer-tdp.md)保留前序身份。下面的阶段表格均为历史测量；生成物与失败候选源码不入库。

## 当前采用的 N12／T16 布局

法线在 Vertex 原有顺序 pack 槽中共用一个原精度量化器，直接形成最终 S12F10 的 N36；准备链不再做第二份格式转换。高18位系数用独立同步银行与低36位同时读，删除高／低字二次读取、标签和等待。生产、存储、消费者与所有包末项均已切换为 V5／T16，详见[数据结构和逐拍契约](vliw-integration.md)。原 Vertex 位置计算、乘加和控制 ROM 未改。

实际量化器两模式共524288码与冻结数学参考逐码相同。前半与完整小兔子 UV／RGB 回放覆盖两种原语布局、随机 CE、导出和内存暂停；准备链覆盖在途复位。完整 box 与948面兔子另用真实当前 MC 及独立引脚模型检查所有有效数值事件和400384字节最终内存。总拍包含初始装填和最终 flush，不是单独 II4 的推算。

分列结果池／独立Z的早期否决已更正。同源重新综合复现旧结果；仅交换两个实例声明顺序、保留连线和行为，就使未改前半的面积差由增加变为减少。独立前半可达源逐字相同，独立后半固定注册接口也确实更小，说明整链存在可复现的全局顺序敏感映射，不能把未改前半的增加当成必要算法成本。综合LUT／ALU估算与PnR Logic分开记录，教育版综合网表加密，未声称已读取每个LUT函数或确定工具内部启发式。

进一步删除重复像素游标、发布计数和sink相位／lane镜像后，结果池方案取得实际整机收益，已采用。Z直接在真实C接受时读取并供D消费，9位分列写、源同步Q所有权、稀疏lane跳过及原有故障边界均完成配套，见[当前契约](vliw-integration.md)。

normal后继已完整实现ACC高字直写、只保存C/det高字、重复配置读／元数据及空emit删除、精确sticky化简和完整floor producer。精确／floor的小SSRAM版、floor的BSRAM版及既定两种实例顺序已定价；完整回放验证数值，floor另重算独立Lighting金值。当前Logic目标下仍选择已有精确N12加新结果池，后继路线未默认采用；结论、源身份和关键报告保留，未采用源码快照与生成中间物已按用户要求清理。没有再用一个局部函数试验代表完整路线。

本轮采用点是单次 P0、54MHz 注册事务边界的独立 GPU 结果，包含555个探针 FF，排除物理 MC、CPU 和 Display。场景回放使用约49.5／99MHz功能时钟；不代表 CPU＋GPU 合体或板测。

## 保留的 Color 完成所有权

The closed VLIW Color path writes its completed packed accumulator directly to
the existing final sample bank. That bank accepts every completion on the common
compute enable. The intermediate sixteen-entry result RAM, pointers, occupancy
and reservation counters are removed; arithmetic, tap-group rules, pixel keys
and final-bank ownership remain unchanged. Generic Color keeps its FIFO and
independent output-backpressure contract.

The completion token, key and accumulator hold together during CE pauses. On an
enabled edge the final bank consumes old accumulator Q before the next group can
update it. An assertion checks the closed acceptance obligation; parent reset,
abort and fault suppress writes. The maintained `vliw_color_direct` miter compares
the complete result sequence to the generic reference with multi-group pixels,
CE pauses, in-flight reset and malformed-packet faults, in portable and native RTL.

The current receipt compares fresh box and full 948-triangle bunny replays using
the current memory transport. Both numerical events and all final memory bytes
match; total cycles do not regress. The earlier preparation receipt used an older
memory-transport baseline, so its cycle totals are not the Color comparison.
Area/timing remain a single-placement independent-GPU result without physical MC,
CPU or Display; this is not board or CPU-plus-GPU qualification.

## 保留的准备与光照数据流

R generates geometry-only step rows while I computes the bounding box. Once the
box is available, R restores the aligned origin with its existing work/step
ports and ALU, then generates only the root support rows it needs. I no longer
computes or exports the origin; its old ROM loop, duplicate coordinate logic
and R's external work-pool write selection have been removed. A rejected final
CHECK ends scanning directly. Zero-bit origin slots still pay their control
cycle; synchronous reads, retirement and CE stalls remain explicit.

The selected version also retains exact 4/2/1 coefficient downshifts, four-bit
Y tile search, and Normal/NDC memory Q ownership through Lighting acceptance.
The latter removes the normal shadow and row-data mux while preserving the
READ/X/CONTEXT/PIXEL calendar. Vertex, precision, RAM/DSP allocation and pixel
order are unchanged. Address specialization, ones-complement quantization and
delayed PA writes remain excluded.

The preparation stage now shares one 53-bit work register between the disjoint
geometry-magnitude and Delta lifetimes. Fixed bit-reversed wiring lets Delta
normalization use the same 1/2/4-bit right-shift window; the old separate work
register and shift path are removed. Quantization, rounding, ROM timing and
the external interface are unchanged. The backend instance is declared before
the frontend with identical named-port connections, preserving the measured
synthesis ordering. Controlled pairs showed that this ordering affects whole
design mapping, so the total Logic reduction is not solely a register-sharing
gain. The current receipt binds the actual production export, its new P0 fit,
and box/full 948-triangle bunny replays with identical numeric events, event
cycles, queue counters and final memory. The parent receipt remains in Git
history; no experiment source snapshots or generated outputs are tracked.

The receipt is a single-placement independent-GPU result, including probe FFs
and the SDRAM transaction wrapper, excluding the physical MC, CPU and display.
Scene cycles use the actual MC model and include startup through final flush.
They are not board or CPU-plus-GPU qualification. The maintained origin smoke
test is `vliw_prepare_chain`; the Lighting handshake regression is
`vliw_light_tuple`. See [complete-model performance](full-bunny-performance.md)
for workload details and reproduction.

## 采用的结构

前半先完成一个 meshlet 的全部 Vertex，再按64位字内三组三角索引完成 Triangle。R/I 固定返回直接写唯一结果池，后半按独立游标读取；删除旧13字导出、后半再收集和重复 payload 银行。T 局部换包只等本 T 算术返回排空，全局配置则等待整个 GPU 和内存服务排空。

Final 用两项旧态容量切断输出 ACK 到输入信用的组合反馈，空队列时保留 BASE 直通。单结果槽虽然过时序，却使 box 总拍退步16.64%，因此没有采用。有效优化不能用频率提高掩盖端到端吞吐退步。

结果池进一步只保留一个六位完成前缀。正式生产者相对 quad 接受的有效沿为 G14、N13/17/21/25、每 lane 三个 W14/15/16 再加 `4*lane`；RGB 的 L15，UV 的 L50。RGB 最后 W、UV 的 L 各推进一次完成前缀；UV 同沿以旧前缀低五位写 LOD，发布仍观察旧前缀。RGB 不写 LOD，Sample 按槽内 RGB 位跳过，异步表 Q 本身没有新增读门控。

生产者模式在全部旧返回完成后才可能随下一 T 的配置改变。CE 暂停冻结整套日历；24个活动槽、8个未发布槽和消费者退休规则不变。旧 G/N/W/L、标签、模式及完整发布条件留作非综合断言，逐物理沿核对。此池只服务闭合的固定顺序生产者，不再承诺任意缺失、乱序返回或提前换模式的运行时硬件诊断。没有改变 Vertex 或精度，也没有把必要工作转移到 T 或下游。

## 历史阶段4拟合

数字和原始报告身份只以当前关键证据为准：同一完整边界、GW2AR-LV18QN88C8/I7、54MHz、两套布局，setup/hold 均为零。统计包含549位动态输入和六个输出观察寄存器，共555个探针 FF；不是无探针核心面积，也不是外部 SDRAM 控制器的时序资格。

完成前缀压缩的同口径净收益为45 Logic、20 FF，BSRAM、RAM16、DSP及实际总拍不变。两套布局均通过约束，不能选其中较高的频率作为唯一代表。

以下用于定位成本，取自同一整体综合的层次 XML。逻辑估算为 `LUT+ALU+6*RAM16`，不是各组件独立 PnR，也不能强行加成最终 PnR Logic；共享 DSP 记在物理实例所在的 compute 等消费者下。

| 部分 | 综合逻辑估算 | FF | BSRAM | RAM16 |
| --- | ---: | ---: | ---: | ---: |
| meshlet 与共享 V/T | 1646 | 633 | 6 | 25 |
| 光栅器 | 1010 | 295 | 3 | 24 |
| I 准备控制／ROM | 1134 | 243 | 1 | 0 |
| I compute，含共享准备 DSP 与 LOD | 1741 | 474 | 1 | 54 |
| 唯一结果池与 NDC | 661 | 148 | 4 | 52 |
| Lighting | 1869 | 1263 | 3 | 21 |
| 最终 LOD 解码、坐标与队列 | 292 | 91 | 0 | 15 |
| 采样系数、成员与包池 | 537 | 431 | 2 | 9 |
| 纹理需求／缓存 | 480 | 212 | 7 | 3 |
| Color | 303 | 52 | 0 | 9 |
| Final | 192 | 78 | 0 | 0 |
| ROP／framebuffer cache | 1041 | 572 | 4 | 0 |
| 输入装填与三端口路由 | 388 | 209 | 0 | 0 |

表外还有父级胶合、Light/Sample 结果银行与探针。BSRAM 总体为前半11、结果池4、Lighting3、Light/Sample结果2、采样包与系数2、纹理缓存7、ROP4；装填器和路由没有额外 payload FIFO。DSP 按物理原语计费，不能把组合体误当一个18×18槽。

## 历史阶段4端到端性能

box 用例只提交 `box_x_minus` 一个面，九顶点八三角，覆盖24400个像素样本，约占400×240屏幕的25.4%；不是封闭box的六个面，也不是全屏填充。兔子使用真实的一个64顶点、93三角 meshlet，变化 UV 来自程序生成。兔子压力把相同几何再按 RGB 解释，以覆盖旧结果尚存时换局部模式和真实 LESS 拒绝。它们不是完整兔子或完整场景的一帧，不能换算成整场景帧率。完整400384字节内存比对包括未触碰区域，不表示全部屏幕像素经过着色。

box 是填充／后半压力：6292个 quad，仅1个空 quad，结果池峰值24，满池有效沿31300。后半吞吐和内存服务确实回压前半，不能用 R/I 的16CE稳态替代总拍。兔子压力有418个 quad、108个空 quad、704个有效像素，峰值槽17且满池计数为零，不能把其长间隔归因于结果池容量不足；小任务的 V/T、准备、遍历和切包成本需要一起考虑。

真实主仓仲裁器、bridge、控制器及独立引脚模型的便携回放另测兔子与 box，两页装填、CE0返回、脏 flush 阻挡配置和全部400384字节物理内存均通过。最后优化与冻结基线的完整 front/back/内存和总拍一致。当前采用点没有重跑整链原语 MC；原语证据由阶段3两页兔子与当前池／后半的有界双模型回放分层提供，不能合称当前整链原语回放已执行。

## 整机预算与边界

除 Logic、BSRAM、DSP 外必须看 CLS。当前独立 GPU 已占约八成 CLS，不能把独立 CPU 与 GPU 的 Logic 相加后小于20736就宣布可放下。CPU 的历史独立报告也不能代替当前整机的重打包、时序及共享服务结果。

本批没有替换 CPU 系统中的 GPU 占位模块，没有加入 CPU 驱动、命令处理器或显示接线，也没有板测。实际引脚仿真使用约49.5MHz逻辑／99MHz控制器，仅是功能证据；最终 GPU 事务边界的54MHz资格与外部内存控制器资格分开。源码、契约、整数参考、RTL、PnR和物理板六层证据不能相互替代。
