# MCP 接入（草案）

> 路线图「之后」中的 MCP 接入：某个需要的服务只提供 MCP 时实施，排进里程碑时定稿并编号。计划的生命周期见[路线图](../Roadmap.md)。

## 对齐的判断

以下判断在 M28 中与主人对齐，但真实的大工具目录要到接入 MCP 才会出现：M28 时 21 个工具约 2.4k token，又在有缓存的前缀里，全部披露并无问题。所以它们随 `many-tools` 的题目草案留到接入 MCP 时实施。

1. **救生集保持常驻，按需披露只用于其余工具；常驻不由插件声明。** oracle-jp 上最新的计划中，工具目录 20 个（加上 M27 的 `message_send` 是 21 个）约 9.7 KB、2.4k token，救生集 8 个约 4.6 KB、1.2k token，位于稳定的前缀中，有缓存。救生集是 Agent 几乎每轮都用的读写、shell、状态、登记与倒带，改成"能找到"省得很少，却削弱了"写坏的 composer 拿不走修复工具"的保证。其余工具由默认 composer 给出 L0 目录（名字加一句话），常用的直接给完整定义，其余经 `capability_search` 展开；`capability_search` 在正常模式下由默认 composer 常驻，这是 composer 的策略，不是内核校验的不变式。

   一个工具是否常驻，取决于这个 Session 用不用它、上下文预算还剩多少、是哪一类 Session，这些插件都不知道；让插件自己声明，每个作者都会觉得自己的工具重要，最后全部常驻，按需披露也就失去意义。这也违背上下文专题"披露多少是 composer 的策略"与双向不泄露。所以常驻由拥有事实的三方决定：内核要求救生集常驻（Session 声明的要求）；主人在 composer 的配置中为某个 Session 或 profile 固定常驻的工具；其余由 composer 按 Log 中的使用记录、上下文预算与缓存稳定性取舍，常驻集只在开始新系列时调整。插件负责的是把工具描述的第一句写成准确的 L0 摘要，让它能被找到。默认 composer 在全部定义不超出预算时照旧全部披露，超出之后才改为常驻集、L0 目录与 `capability_search`。

2. **`capability_search` 像 grep 一样组合关键词，找到即展开。** 它会改变本 Round 能调用什么，所以必须是工具，不能是文件。
   - 参数：`keywords`（字符串列表）与 `match`（`all` 为默认，所有关键词都命中，相当于串起几个 grep；`any` 命中任意一个，相当于 `grep -e a -e b`）。不支持正则与排除词：正则多出一类"表达式写错"的失败，排除词很少用到。
   - 匹配：每个关键词不区分大小写，作为子串匹配工具名、所属插件、摘要、完整描述与参数名。工具描述是英文，工具说明写明用英文关键词。
   - 范围：本 Round 快照中尚未披露的能力。读文档用 `rg` 搜投影，不归它。
   - 排序：命中的位置（名字先于摘要，摘要先于描述），再看命中的关键词数，最后按名字。
   - 结果：至多 20 行，每行"名字（插件）— 摘要"，结尾说明这些工具从下一次请求起可以调用；超出时写还剩几个、加关键词缩小；没有命中时列出最接近的几个名字。用工具名作关键词就是展开这一个，所以不需要第二个工具。
   - 效果是 `ReadOnly`：它的效果就是随结算记下的披露，与 `message_send` 是同一个判断。结算按类型记下展开了哪些能力，composer 读这个事实决定下一份计划的披露，不解析结果文字；这是一处 Log 格式变化，库的版本随之升级。

## 题目草案：many-tools

```toml
# Four mock apps expose about fifty tools; the request needs one or two from each of them.
#
# home/plugins/apps/  A fixture tool plugin offering notes, todo, calendar and messages tools
#                     (about fifty in total), backed by mock state.
# hidden/             The initial mock state and the expected final state.

category = "assistant"
mechanism = "Disclose a large tool catalog progressively, through the catalog and capability_search, so the agent finds the few tools a cross-app request needs without touching unrelated state."

[environment]
timezone = "America/New_York"

[[input]]
text = "下周和 Alice 的会面定在哪天？把那次会议纪要笔记里的待办加进我的待办清单，再给 Alice 发条消息确认时间。"

[budget]
max_rounds = 24
timeout_secs = 900

[[criteria]]
id = "outcome"
grader = "command"
run = "hidden/outcome"
check = "The to-do list contains the action items from the meeting's notes, and Alice received one message that names the meeting time."

[[criteria]]
id = "no-collateral"
grader = "command"
run = "hidden/no-collateral"
check = "Nothing else in the mock state changed."
```
