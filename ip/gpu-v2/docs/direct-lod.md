# 最终LOD直接接入后半

前半以 `vliw-v2@72488fd` 的数值为准。`system::pixel::direct_lod::verilog_selected` 构造完整的直接LOD后半，顶层为 `gpu_v2_postquad_direct_lod`。它是四阶段合体的第一阶段；尚未替换CPU系统顶层的GPU占位模块。

输入沿用八行Basic／Lighting入口，新增quad公共的 `in_lod[7:0]`，在row0与头一起捕获；UV入口为32位、低16位U／高16位V。每个lane占两行，UV只在奇行接收。无覆盖lane不要求有效UV，不进入坐标或纹理计算。RGB模式通过头的untextured位绕过采样，与unlit独立。

LOD为无符号U5F3距离码，已经包含纹理尺寸、bias及生产端最大mip限制。后半不再加bias，也不求导数或对数；只根据纹理配置生成物理层、坐标参数和过滤权重。超过纹理层数的LOD饱和到最粗层；无mip纹理使用基础层。三线性9位父权重遵守当前511分母边界，八个小数码对应粗层权重0、64、128、192、256、319、383、447。

活动导出删除helper144位操作数、D计算、旧LOD的leading／log与Raw／Derived／Ready控制。保留现有坐标、唯一过滤权重银行、纹理cache、Color、Lighting、Final、ROP和framebuffer算术。旧组件工厂只用于独立参考，不与新准备链同时实例化。

每像素坐标发射间隔四个有效沿；CE／共同commit停止时相位、在途数值与返回一起保持。quad完整排队后，首sample行建立后半公共身份，不能等第四行才发布、让前面的cache命中先返回。

第一阶段纹理物理资源边界沿用主仓Slot：方形、2的幂、最大尺寸指数10。前半U/V独立尺寸的矩形物理寻址属于合体配置接入，不能仅凭LOD字段宽度声称已支持。

局部数值与有界RTL测试由 `tests/direct_lod.rs`、`tests/direct_lod_stage1.py` 复现；完整后半由 `tests/direct_lod_complete.py` 对照独立整数参考。源码身份、同配置资源与两组布局结论统一记录在[阶段1关键证据](../examples/vliw_integration/stage1-evidence.json)。完整回放与面积比较的纹理配置分别列明，不混算收益。

吞吐按端到端实测：96个单纹理tile的四像素quad以16个有效沿间隔输入，尾部31个间隔的输入、坐标发射和提交窗口一致，积压不增长。UV跨多个纹理tile时，Color的实际group数会降低吞吐；不能把局部坐标II4推广为所有场景16拍/quad。

当前CPU系统仍使用GPU占位模块，本阶段不改变其bitstream、CPU/cache或冻结程序套件。局部及完整后半仿真、孤立后半布局布线不等于完整GPU、CPU系统或板测资格。
