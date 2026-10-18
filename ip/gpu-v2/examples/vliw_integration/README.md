# VLIW 合体验证

本目录保存四阶段 GPU 合体所需的独立整数参考与验证入口。所有仿真均有沿数上限和进程超时；生成物写入 `target/gpu-vliw-integration-20261018/`。不需要 E 盘工作树、旧实验缓存或上板环境。

阶段3入口见[主仓内存接入合同](../../docs/vliw-memory.md)与[关键证据](stage3-evidence.json)：`tests/vliw_sdram.py` 使用真实主仓 arbiter／bridge／controller 与独立 SDRAM 引脚模型，分别运行 portable 兔子+box、native 两页兔子；源清单相同，负载与功能时钟边界分别记录。

## 阶段1：最终 LOD 直接接入完整后半

`reference/` 仅包含 `vliw-v2@72488fd` 的 V/T 数学、当前 I 数学、场景记分牌及两个原始输入：真实 Stanford bunny 的一个 meshlet 和 box 的一个面。`origin.json` 绑定来源提交、原始夹具与各文件哈希。软件夹具采用仓库LF换行并在采用哈希中绑定，原始哈希仍保留上游身份；Vertex等采用算术源通过 `.gitattributes` 保留原始字节。UV 是程序生成的变化坐标，不是兔子原贴图。未消费的旧 P/H、Vertex/Triangle 结果及近面缓存已删除。

```powershell
cargo test --release -p gpu-v2 --test direct_lod -- --nocapture
python ip/gpu-v2/examples/vliw_integration/prepare_uv_inputs.py
python ip/gpu-v2/tests/direct_lod_complete.py --tag replay-new --mode both
python ip/gpu-v2/tests/direct_lod_complete.py --tag paced-new --steady --single-texture-tile --paced --count 96 --mode portable
```

每次回放使用新 tag；整数输入生成器的默认目录必须尚不存在，可用 `--out` 指定新目录。工具通过 `IVERILOG_EXE`、`VVP_EXE` 和 `GOWIN_HOME` 指定。Cargo 与其它线程并行时应使用独立 `CARGO_TARGET_DIR`。

`prepare_uv_inputs.py` 现场从 V96、uniform、配置和三角引用重算整数输出。生成的 `quads.jsonl` 以自然 lane 0/1/2/3 记录几何及数值通过 mask、S12F10 法线、S18F16 容器 UV、Q8.8 深度与最终 U5F3 LOD；另保留 Gray 0/1/3/2 顺序的13个运输字及逐字比较 mask。被拒 lane 的深度 payload 不属于有效发布语义。

后半必须消费这里的最终 LOD，不能重新计算旧 helper 导数、旧 bias 或旧 LOD。当前 I 使用 h30、公共 E 对齐 D32、R18 倒数及凸夹紧 F16 权重；正 D 对应的 Z32 量化为0仍可有效。没有8199近面拒绝。

`audit_stage1.py` 只读复核已生成的冻结来源、完整回放及原始 P0/P1 资源报告；输出 `stage1/independent-audit-final.json`。来源清单为 `stage1/source-final.json`。完整整数回放使用 n5 纹理配置；资源比较另以 n6/base0x1000、400×240 framebuffer 的共同配置导出，二者只允许声明过的三处常量差异，不能混作逐字节相同 RTL。

96 quad 定向用例按每16个外部有效 CE 提供一个输入，使用同纹理 tile、全覆盖、Lighting与纹理均开启。最后31个间隔的输入、C发射及提交窗口均为496CE，提交相邻间隔允许15/17CE抖动；这是该有界热负载的平均16CE/quad，不代表任意纹理工作集、冷缺失或整帧吞吐。

阶段1不依赖新的前半、结果池或阶段2 wrapper。阶段2专用脚本即使存在于开发工作树，也不构成阶段1的采用资格。

## 阶段1证据边界

完整后半用例核对自然 lane 身份、实际最终 LOD、逐 tap Color、Final、ROP 的565量化及全部 framebuffer 字节；包含无覆盖输入、稀疏 mask、CE暂停与真实128字节事务。Lighting包含定向轴法线，深度测试为 ALWAYS，draw 在排空前保持不变。这不是任意光照、真实遮挡、多draw全局所有权、完整 GPU/CPU/display、物理内存控制器或板测证明。

历史 selected 基线和前半单独回放的收据保留在独立子目录，不能替代新的完整合体资格。资源总数包含真实探针寄存器；阶段1新探针411个，旧基线407个，不混用或默扣。

## 阶段2：正式前半、唯一结果池与完整后半

`verify_stage2.py` 使用 `system::vliw::verilog` 的正式整体连接，不另造一套永久 wrapper。`stage2_tb.v` 为有限内存对端；`main.rs` 是独立 Lighting 整数金值工具，从真实 S12F10 法线和像素中心 NDC 计算漫反射结果，并抽样对主仓 counted 模型交叉核验。

```powershell
python ip/gpu-v2/examples/vliw_integration/verify_stage2.py --tag full-box-new --name box_x_minus --mode both
python ip/gpu-v2/examples/vliw_integration/verify_stage2.py --tag full-rabbit-new --name bunny_patch_1 --mode both --stress --rgb-repeat
python ip/gpu-v2/examples/vliw_integration/audit_stage2.py --tag full-box-new --tag full-rabbit-new --output target/gpu-vliw-integration-20261018/stage2/independent-review
```

输入保留真实位置、法线和索引；I 的纹理尺寸配置显式改为6，与实际64×64纹理及完整 mip 链一致。`--rgb-repeat` 将第二批相同 geometry/attr32 按 RGB 解释，保持全局 draw 参数不变，定向检查旧结果仍在池中时的模式切换及相同 Z 的 LESS 拒绝。程序 UV 不是兔子原贴图。

记分牌独立检查全部 Vertex 五项、16项 Triangle 包、h30、自然 lane 的 I 属性/Z/最终 LOD、实际 Lighting、采样、Final，以及纹理和两帧缓冲共400384字节。`transport.py` 在冻结数学参考之后打包 V5／T16，原数学参考不改。被拒 lane 的未承诺深度 payload 不作有效值断言。空 quad 必须安全跳过，不能产生 Lighting/Sampling/Final 输出；24个全局槽及8个未发布槽分别检查。

所有仿真均限制最大 wall 沿和进程时间。压力包含 CE/Vertex 导出暂停、有限内存长暂停与 CE0 付费返回。ROP 的 `commit_valid` 是 wall 事件，不能再以 CE 门控丢失；其 mask 是输入 mask，真实深度拒绝由独立逐像素比较及最终 framebuffer 金值证明。

本阶段固定 diffuse、specular=0、alpha255、blend关、LESS及深度写。任务总沿包含开机装表、DMA、全部V/T、R/I、后半和 flush，不能当作整帧性能或任意工作集稳态吞吐。物理 SDRAM 仲裁器、CPU 驱动、显示和板测另属后续阶段。

## 当前采用点与维护测试

[布局关键证据](layout-evidence.json)是当前独立 GPU 采用点：法线提前按原精度量化并合成 N36，T 的高18位独立同步读，V5／T16 全链路替代旧运输。数据结构和逐拍契约见[合体实现](../../docs/vliw-integration.md)。[Color 收据](color-direct-evidence.json)保留直接基线，[准备收据](frontend-preparation-evidence.json)、[TDP framebuffer 收据](framebuffer-tdp-evidence.json)及阶段收据保留历史身份。源码 SHA 按 CRLF→LF 归一化；本轮没有修改 Vertex 乘加与算术 ROM。静态 II2 ROP、独立维护和简单预取见[帧缓冲合同](../../docs/framebuffer-tdp.md)，证据边界见[合体资格](../../docs/vliw-qualification.md)。

```powershell
python ip/gpu-v2/tests/vliw_normal_quantization.py --tag normal-rne-new
python ip/gpu-v2/examples/vliw_integration/verify_frontend_stream.py --tag transport-new --name bunny_patch_1 --mode both --stress --normal-mode 16
cargo test -p gpu-v2 --test vliw_prepare_chain -- --ignored --nocapture
```

标量测试穷举两个模式共524288码，包含 normal16 前级截位、回绕、半值取偶与饱和；源运输暂停和准备链复位／CE检查另由两种物理布局的小回放覆盖。向下截断仅作独立误差候选，未进入默认 RTL。

当前结果池采用固定DI分列写和独立Z银行：φ1直接存Z，sink实际接受C时同步读，D直接消费Q。属性相位／像素地址复用、发布游标、sink相位与lane均由唯一权威维护；删除旧深度暂存／预约／返回标签、重复valid和镜像计数。受控复审与完整normal后继的存储／精度比较见布局证据；不能把未改模块的综合映射变化当成新增算法工作。

结果池只接正式固定日历：G14、N13/17/21/25、W14至28、RGB L15／UV L50。小后半回放曾使用人工提前返回，现已修正；新旧该微测的总拍不可直接比较。持续16CE供给和真实box／rabbit的原时间轴保持不变。

```powershell
cargo test -p gpu-v2 --test vliw_backend -- --nocapture
cargo test -p gpu-v2 --test vliw_quad_pool -- --include-ignored --nocapture
python ip/gpu-v2/tests/vliw_backend_complete.py --tag current-small-new --mode both
python ip/gpu-v2/tests/vliw_backend_paced.py --tag current-paced-new --mode both
python ip/gpu-v2/tests/vliw_backend_failstop.py --tag current-fault-new --mode both --fault final
```

每次回放使用新tag，避免覆盖保留证据。`VLIW_POOL_TEST_TAG`可指定池矩阵输出目录。故障脚本另外支持texture与abort，全部回放都有有限沿上限和宿主超时。

`audit_stage2.py` 默认只审指定合法回放；故障资格需要显式传入同源 `--error-contract` 聚合收据，不自动借用历史版本。`audit_stage1.py` 是在对应阶段冻结源上审查保留工件的历史工具，不是从空 `target` 开始生成资格的入口。

## 完整模型回放

`full_bunny.py` 将官方 Stanford ASCII PLY 的全部面恰好提交一次，划分为最多64V／96T 的 meshlet。全模型统一生成位置与法线，保持跨 meshlet 的重复顶点一致；真实 SDRAM 服务交替装填 V96／index 两页，最后排空后半并 flush。负载和证据边界见[完整兔子性能](../../docs/full-bunny-performance.md)。默认使用 portable 原语，不代表板测。

```powershell
python ip/gpu-v2/examples/vliw_integration/full_bunny.py --ply PATH/bun_zipper_res4.ply --tag complete-front-new --camera front --max-cycles 2000000 --sim-timeout 600
```

`--camera side|back` 选择其它机位。运行前从官方 bunny 压缩包提取所需 PLY，并显式设置沿数／时间上限。948面结果不代表69451面原始模型。使用新 tag，生成数据、网表与画面保留在忽略的 target 目录。

[Color 收据](color-direct-evidence.json)保留前一采用点；通用 Color 参考仍有 FIFO。闭合工厂与双布局结果序列回归使用 `cargo test --release -p gpu-v2 --test vliw_color_direct -- --include-ignored --nocapture`。准备链与光照握手回归分别为 `cargo test --release -p gpu-v2 --test vliw_prepare_chain -- --ignored --nocapture` 和 `cargo test --release -p gpu-v2 --test vliw_light_tuple -- --ignored --nocapture`。
