# VLIW 合体验证

本目录保存四阶段 GPU 合体所需的独立整数参考与验证入口。所有仿真均有沿数上限和进程超时；生成物写入 `target/gpu-vliw-integration-20261018/`。不需要 E 盘工作树、旧实验缓存或上板环境。

## 阶段1：最终 LOD 直接接入完整后半

`reference/` 仅包含 `vliw-v2@72488fd` 的 V/T 数学、当前 I 数学、场景记分牌及两个原始输入：真实 Stanford bunny 的一个 meshlet 和 box 的一个面。`origin.json` 绑定来源提交、原始夹具与各文件哈希。UV 是程序生成的变化坐标，不是兔子原贴图。未消费的旧 P/H、Vertex/Triangle 结果及近面缓存已删除。

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

## 证据边界

完整后半用例核对自然 lane 身份、实际最终 LOD、逐 tap Color、Final、ROP 的565量化及全部 framebuffer 字节；包含无覆盖输入、稀疏 mask、CE暂停与真实128字节事务。Lighting包含定向轴法线，深度测试为 ALWAYS，draw 在排空前保持不变。这不是任意光照、真实遮挡、多draw全局所有权、完整 GPU/CPU/display、物理内存控制器或板测证明。

历史 selected 基线和前半单独回放的收据保留在独立子目录，不能替代新的完整合体资格。资源总数包含真实探针寄存器；阶段1新探针411个，旧基线407个，不混用或默扣。
