# 订阅模板与 Clash 分流规则

Web「模板 → 添加模板」默认填入可编辑的 Clash/Mihomo 示例，名称单独填写。
保存后可预览、生成、编辑和回滚。无需填写 `apiVersion`、`kind` 等 V3 外壳。
已有 V3 模板保持可读可编辑；原生 Clash 模板的输出目标为 `mihomo`。

只写分流规则时，可以粘贴：

```yaml
rules:
  - DOMAIN-SUFFIX,example.com,DIRECT
  - IP-CIDR,192.168.0.0/16,DIRECT,no-resolve
  - MATCH,PROXY
```

也可只粘贴以 `-` 开头的 YAML 规则列表。未填写 `proxy-groups` 时，服务端
自动补入名为 `PROXY` 的选择组，以 `include-all-proxies: true` 引入节点池。
完整默认配置见 [clash-routing.yaml](../../examples/templates/clash-routing.yaml)。

需要自定义时，填写 `proxy-groups`、`rules`、`rule-providers`、`dns`、`tun`。
代理组支持 select、url-test、fallback、load-balance、显式成员、组间引用、
include-all-proxies、filter、健康检查参数等原生选项。规则顺序、no-resolve、
逻辑条件、规则集和 DNS policy 的顺序会保留。允许有界的 YAML anchor/alias
及 `<<` 组默认值复用；不允许脚本、自定义 tag、重复 YAML 键。
格式依据：[Mihomo 规则](https://wiki.metacubex.one/config/rules/)与
[代理组](https://wiki.metacubex.one/config/proxy-groups/)。

节点凭据在节点池维护，模板不接受内嵌 `proxies`、外部 `proxy-providers`
或路由范围之外的全局配置。规则目标使用已定义代理组或内建策略；显式节点
成员在生成时检查，已知失效节点会移除并给出警告，未知名称仍报错。重名节点、与代理组/内建策略同名的
节点会使用 ID 后缀输出，节点池中的显示名称保持原样；重名解除后，已选节点的
原 ID 后缀引用（含二次碰撞序号）仍可解析，现有精确名称优先。

保存会检查基本结构、常见 CIDR/NETWORK 参数、策略/规则集引用、重名与循环。
自动分组的 filter、exclude-filter、exclude-type 在服务端有界求值，每次生成
输出实际成员列表；支持常见正则及 look-around，无法处理或超出计算预算的
表达式明确报错。DNS/TUN 及高级选项仍以实际 Mihomo 校验为准，不把服务端
检查当作完整客户端 schema。例如 inline 规则集
不被仓库固定的 Mihomo v1.19.0 校验器支持；默认示例及 file 规则集场景有
该版本的真实客户端验收。规则集由客户端读取/下载，服务端不执行规则集 URL。

每次成功保存创建独立历史版本；并发保存依提交顺序成为活动版本，所有成功
提交的快照都会保留。回滚后再保存使用历史最大编号加一。历史可继续加载
100 条以外的记录，编辑直接读取活动版本。校验失败保留输入与旧版本，生成
失败保留最近成功产物；主动删除源前的产物不能作为回退。固定版本订阅回退缓存时仍受版本固定约束。
多个订阅共用模板时，各自需要的最后成功产物都会受到保护，不会因其他订阅
生成而被淘汰；重复刷新只保留有界的额外缓存。删除或调整订阅后，旧产物在
后续生成时恢复为可回收状态。

模板及订阅的节点选择范围约束全部代理组；显式成员或自动分组不会带入范围外
节点。范围外的显式引用会给出警告；有其他可用节点时，筛选后为空的 Mihomo
代理组保留名称并明确阻断（select + REJECT），其余分组继续可用，规则目标
不会悬空或自动改为直连。没有任何可用节点或使用不支持的组类型时明确报错。
本次安全修复会按新规则重建旧缓存；无法重新生成时返回明确错误，避免继续
分发旧缓存中未选中的节点或无效代理组。修复后成功生成的产物仍支持故障回退。

正在被订阅引用的模板不能删除，界面会提示先解除引用。关闭或切换模板窗口
后，迟到的编辑、历史、预览请求不会覆盖新窗口；更改生成参数会清除旧结果。

设计归属 [M5](../plan/milestones/M5-generator-and-v3-template.md)，接口归属
[模块边界](../contracts/module-boundaries.md)，验证见
[并发功能矩阵](../acceptance/functional-matrix.md)。
