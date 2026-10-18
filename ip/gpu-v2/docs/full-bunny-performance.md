# 完整兔子性能回放

当前独立 GPU 和内存链由[布局关键证据](../examples/vliw_integration/layout-evidence.json)限定；[Color 收据](../examples/vliw_integration/color-direct-evidence.json)保留本轮直接基线。模型为官方 Stanford `bun_zipper_res4.ply`，453个源顶点、948个三角形，全部面恰好提交一次，没有进一步减面。它是早期小夹具对应减面版本的完整模型，不是69451面的原始扫描。Vertex 乘加、位置计算与最终数值精度不变；法线量化提前，运输格式变为 V5／T16。

## 负载和计时边界

400×240，单面，正面机位距模型约2.8，焦距180像素，最长模型尺寸缩放为1.5。统一位置网格及累积顶点法线保证跨 meshlet 的重复顶点一致。UV 和64×64 mip纹理由程序生成，不是扫描材质。漫反射开启、高光为零、alpha255、混合关闭，采用 LESS 和深度写。

全模型分成17个 meshlet，各不超过64V／96T；边界复制后实际执行670个顶点。payload10648字节、metadata544字节、配置128字节。两个输入页让后继 DMA 与当前 meshlet 重叠，后半不逐 meshlet 排空。

计时从第一条装填 descriptor 到最后后半排空和脏 framebuffer flush。包含 SDRAM 启动和真实内存服务；启动不是每帧重复的 DMA 成本。ROM 装表和预清显存不计。实际 arbiter、bridge、controller 和独立引脚模型均在，未加入 CPU／显示争用。功能时钟约49.5／99MHz；换算54MHz仅是拍数缩放，不是板测或 MC 重新取得时序资格。

## 当前正面机位

| 项目 | 结果 |
| --- | ---: |
| 总墙钟拍 | 292662 |
| 三角接受／背面拒绝 | 417／531 |
| 遍历节点 | 10653 |
| quad／空quad | 3559／532 |
| 覆盖像素样本 | 7107 |
| ROP深度通过／拒绝 | 6753／354 |
| 满池／R信用等待拍 | 0／0 |

相同当前内存链基线为294680拍，新版减少2018拍。样本包含三角形重叠，不等于最终不同屏幕像素数。空quad跳过昂贵后半，其前半代价仍计入总拍。

box 比较只提交 `box_x_minus` 一个面：八个三角形、24400个覆盖样本，不是全屏或封闭六面 box。当前134952拍，基线134982拍。完整分段、队列计数、源和最终内存身份均在关键证据中。旧侧面机位数据不作为本版结果。

## 验证与复现

回放逐项检查 V5、三角索引与 T16、h、插值属性、Z／LOD、独立整数 Lighting、采样、Final、ROP顺序，以及包括未触碰区域在内的全部400384字节最终内存。有效数值和最终内存与重新测量的当前基线一致。阶段重叠的 busy 计数不能相加当总拍。

```powershell
python ip/gpu-v2/examples/vliw_integration/full_bunny.py --ply PATH/bun_zipper_res4.ply --tag complete-front-new --camera front --max-cycles 2000000 --sim-timeout 600
```

每个 tag 创建新的忽略目录；支持 `side`、`back`，新机位需新测量。输入取自官方 bunny 压缩包的 ASCII PLY，生成夹具、网表、画面和历史快照不入库。高分辨率原始模型只有容量研究，本资格没有其帧率测量。
