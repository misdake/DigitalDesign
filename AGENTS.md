# DigitalDesign-code 项目指南

多 crate workspace:`core`(digital_design_code,电路组件库）、`cpu_macro`(define_isa! 宏）、`cpu_v1`（旧版 CPU)、`cpu_v2`（当前主线，本文件主题）。

## cpu_v2 结构（src/）

- `isa.rs` / `isa.html` — ISA v2.6 定义（cpu_macro 生成编码/解析/Display),**不可修改**。
- `sim.rs` — 指令集模拟器（eval → StateChange → commit)。
- `compiler/` — 编译管线：SSA/CFG IR(builder) → passes(const-prop/CSE/DCE)→ 线性扫描 regalloc → codegen（分支松弛）→ assembler/linker。
- `frontend/` — rcc 前端（syn 解析 → 子集校验 → AST→IR lowering)+ `spec.md`(rcc 语言规范，随实现更新）。
- `rcc_std/` — 标准库（heap/mem/mul/vec，用 rcc 自举；自动并入每个程序，未用函数被链接器丢弃）。
- `dsl_rt.rs` — rcc 内建的宿主侧实现（Ptr/Slice2/addr_of/halt/assert)，让 rcc 程序成为合法 Rust(rust-analyzer/rustc 可读）。
- `dsl_progs/` — rcc 示例程序（`*_dsl.rs` 命名）。
- `bin/rcc.rs`、`bin/rcc-run.rs` — CLI artifacts。
- `tests/`(cpu_v2/tests/)— 集成测试：common(helper)、rcc_basics / rcc_control / rcc_calls / rcc_memory / rcc_errors / rcc_std / rcc_ported。

## ISA v2.6 速记（16 寄存器，哈佛架构）

- 指令/数据存储各 64K;r13=ra(call 系硬件写入）、r14=sp(sp_add/sp_sub/store_sp/load_sp 隐式使用）,**不可分配**。
- 调用约定（编译器实现）：返回 r0–r1，参数 r2–r7,caller-save r0–r7,callee-save r8–r12（按需保存，非叶函数自动保存 ra),tmp=r15（远 call/分支松弛/scratch 专用，不可分配）。
- 立即数限制：j_cc ±128（越界由后端分支松弛处理）、addi 仅 ±1..8 且无 0、cmp_i u4/cmp_si i4、load/store_mem 偏移 i4(−8..+7)、store_sp/load_sp/sp_sub/sp_add u8、移位仅立即数 u4。
- call:3 槽预留，链接器近 `call_rel`/远 `load_lo`+`load_hi`+`call_reg`；间接调用经 tmp + `call_reg`。**不启用 call_abs**。
- 函数间保留 1 个 halt 空隙（反汇编分界 + PC 越界兜底）。

## rcc 语言（详见 src/frontend/spec.md)

Rust 真子集（合法 rcc 即合法 Rust，全部语义 unsafe)。类型：u16/i16/Ptr（数据指针）/fn 指针/bool（仅条件，不可存储）。数组 [u16;N]/[i16;N](`Slice2` 方法 read/write/as_ptr/len，目标机零边界检查）。const/static(数据段，main 入口隐式 `__data_init`)。`addr_of(&x)`：全局=编译期常量，局部=sp+slot。if/while/for/break/continue/if 表达式/短路 && || !。函数指针（LoadFuncAddr + call_reg)。**不支持**:`*`、`/`、`%`、struct、泛型、闭包、match、引用、宏；遇到即带位置报错。标准库函数直接用名字调用（malloc/free/mem_set/mem_copy/mul_16x4/8/16/vec_*)；编译器按调用图可达性在 main 入口自动插一次 `init_heap`/`init_vec`（参数来自 CompilerOptions)。

## 常用命令

本机构建要点：必须加载 MSVC 环境并把 cargo 放到 PATH 前（Git Bash 的 GNU `link` 会抢占 MSVC `link.exe`)。构建环境初始化已内化如下（等价于原 `run_tests.bat`，该脚本已不需要）:

```bat
:: build_env.bat 的内容（直接在 cmd.exe 里逐行执行，或存为 .bat 调用）
call "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
set PATH=C:\Users\misdake\.cargo\bin;%PATH%
```

之后即可正常使用 cargo:

- 测试/静态检查：`cargo test -p cpu_v2`、`cargo clippy -p cpu_v2 --all-targets`
- 构建二进制：`cargo build -p cpu_v2 --bins`（产物在 `target/debug/rcc.exe`、`rcc-run.exe`)
- 编译 rcc 程序：`./target/debug/rcc <input.rs> [-o out.bin] [--lst out.lst] [--no-opt] [--stack-init N] [--data-base N] [--heap-begin N] [--heap-size N] [--vec-cap N]`（数字十进制或 0x 前缀）。`mod name;` 在输入文件旁解析（`<name>.rs`/`<name>.dsl.rs`/`<name>/mod.rs`)。产出三件：`.bin` 二进制镜像（RCC1 头 + u16-LE 指令）、`.lst` 反汇编（函数签名/块角色/调用名/`; line N`)、`.dbg` 调试信息（文件表、函数（地址/帧/变量位置 rN/frame+N/ssa)、全局变量地址、PC→行表，可供假想 debugger 使用）。
- 运行：`./target/debug/rcc-run <input.bin> [max_cycles]` → 打印 halt 信号与周期数。
- 调试：`./target/debug/rcc-dbg <input.bin> [--port 8321]` → 打开 http://127.0.0.1:8321：单页 web debugger。**源代码为主**：源码面板（当前行高亮，点行号按源码行设断点，步进/下一行/继续/重置），反汇编面板为辅（PC 高亮，点行设断点），寄存器+flags，内存查看器（地址/sp/heap/data 快捷跳转），globals（名称/类型/地址/值），当前函数的 locals（rN/frame+N 实时取值，ssa 标记不可见），跨文件源码步进（库函数也能逐行跟）。API：`GET /api/state`、`GET /api/mem?addr=&len=`、`POST /api/cmd?cmd=step|next|continue|reset`、`POST /api/break?addr=<hex>&on=0|1`、`POST /api/breakline?file=<idx>&line=<n>&on=0|1`。
- 看反汇编：编译时 `--lst`；或测试 `test -p cpu_v2 compiler::tests::optimize::test_listing_demo -- --nocapture`;`Compiler::finish` 总是返回 `(Vec<Instruction>, String)`（带函数签名/块角色/调用名/`; line N`)。

## 约定

- 测试必须给 `simulate` 传最大 cycle（防死循环挂死）；期望值尽量用对等 Rust 代码现场计算而非魔数。
- 项目文件只用英文（代码、注释、spec、commit)。
- `docs/` 文件夹已不存在（原 redesign 草稿废弃，现状以本文件与 `src/frontend/spec.md` 为准）；构建命令已内化在本文档。
- 改动由用户决定提交时机（除非用户明确要求提交）。
- 测试布局：pipeline 单元测试留在 `src/compiler/` 各文件内；语言/库/集成测试放 `cpu_v2/tests/` 按类分文件。
