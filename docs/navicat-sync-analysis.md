# Navicat Premium Lite 18 数据传输 / 数据同步 / 结构同步 逆向分析

> 目标程序：`C:\Program Files\Navicat\Navicat Premium Lite 18\navicat.exe`
>
> 版本：18.0.4.0（ProductVersion 18.0）
>
> SHA-256：`47BF955B4F6F6A299612C487425DE60134DBFDE3ABE1450E06CC6CF0E06DF292`
>
> 架构：Windows x64 PE
>
> 分析范围：MySQL、MariaDB、SQL Server、Oracle、PostgreSQL、SQLite 六个引擎的
> **数据传输（Data Transfer）**、**数据同步（Data Synchronization）**、
> **结构同步（Structure Synchronization）**
>
> 分析日期：2026-10-21

---

## 0. 结论摘要

Navicat 把三件事拆成三个独立入口，但底层共用同一套“对象发现 → 元数据快照 →
差异模型 → 脚本生成 → 部署”的流水线：

| 功能 | 解决的问题 | 是否改目标结构 | 是否有逐行预览 |
|---|---|---|---|
| 数据传输 Data Transfer | 把对象/数据从源搬到目标，目标可以不存在 | 可选建表、建索引、建触发器 | 无逐行预览，有 SQL 预览/日志 |
| 数据同步 Data Synchronization | 让两边**数据**保持一致 | 不改结构 | 有：Difference / Insert / Update / Delete / Same / All |
| 结构同步 Structure Synchronization | 让两边**结构**保持一致 | 是 | 有：对象树 + DDL 左右对比 + 部署脚本 |

三者共同的关键点：

1. **对象级选择，不是整库无脑覆盖。** 每个表/视图/函数/序列/触发器都能单独勾选，
   数据同步还能逐表改映射、逐行取消某个变更。
2. **比较项是可配置的。** 结构同步把“是否比较自增值、字符集、排序规则、定义者、
   所有者、表空间、分区、序列、规则、排除约束、触发器、视图、函数……”做成一组开关；
   Navicat 不是简单比较 `SHOW CREATE` 文本，而是先归一化成对象模型，再按开关比较。
3. **比较和部署分离。** 先 Compare 出差异，再生成部署脚本；部署阶段可以继续
   “忽略错误继续 / 事务回滚 / 多条语句一次执行 / 调整脚本顺序 / 在查询编辑器打开脚本”。
4. **数据同步靠“键映射 + 字段映射 + 条件”对齐两边。** 默认只自动映射同名表和同名字段；
   表名/字段名不同时可以手工改；比较范围可以用 SQL 条件限制（例如 `id > 10000 AND id < 20000`）。
5. **数据写入不是一条条 `INSERT`。** 传输支持扩展 INSERT、IGNORE、REPLACE、
   DELAYED、单事务、每 N 条提交、批量大小、源/目标锁表；SQL Server 还专门处理
   `SET IDENTITY_INSERT`，PostgreSQL 需要处理 identity 的 `OVERRIDING SYSTEM VALUE`。
6. **每个引擎的差异落在 catalog、DDL 和特殊类型上，而不是三套完全不同的算法。**
   MySQL/MariaDB 用 `SHOW CREATE TABLE` 作为可选 DDL 来源；SQLite 用 `sqlite_master`；
   Oracle 用 `ALL_*` + `DBMS_METADATA` 重建；SQL Server 用 `sys.*`；PostgreSQL 用
   `pg_catalog` + `pg_get_*def`。

> **Lite 版说明**：安装的是 Navicat Premium **Lite** 18，而随附手册第 17～19 章标题
> 标注为 “Available only in Enterprise and Standard Edition”。二进制和手册里仍包含
> 这三个功能的完整实现与选项字符串，但 Lite 版可能隐藏或限制部分入口。本文分析的是
> “程序里实际存在的逻辑”，不是对 Lite 授权范围的判断。

---

## 1. 方法与证据

### 1.1 REA 绑定目标

REA 的 `open_binary` 成功绑定目标，返回：

- 格式：`pe`
- 类型：`executable`
- 架构：`x86_64`
- SHA-256：`47bf955b4f6f6a299612c487425de60134dbfde3abe1450e06cc6cf0e06df292`

同目录的 `libcc.dll`（134,817,864 字节）承载 Navicat 的 catalog/DDL/语法层，
`libnp2.dll`（24,626,248 字节）承载 SQL 解析器与部分引擎方言，`nparser.dll`
（22,713,416 字节）承载 PL/SQL、SQLite 等解析器。三者用于交叉印证 SQL 关键字和
对象类型。

### 1.2 证据来源

本机没有配置 Ghidra、IDA、Hopper 或 Kuna，REA 的 `binary_session` 返回
`analysis_provider_binding: null`，因此**没有做函数级反编译**。本文的证据来自：

1. `navicat.exe` 内嵌的 **Delphi DFM 表单资源**（`RT_RCDATA`）——用于确认设计器/
   连接/查询等表单结构，以及 `TTableDesign*`、`TIndexDesign*` 等引擎差异；
2. `navicat.exe` 的 **UTF-16 字符串表**——三个功能的入口名、选项名、提示语几乎都在
   这里，是本文最主要的“选项清单”证据；
3. `libcc.dll` / `libnp2.dll` 的 **ASCII/UTF-8 字符串**——`IDENTITY_INSERT`、
   `SHOW CREATE TABLE STATEMENT`、`PRAGMA STATEMENT`、`ALL_TAB_COLUMNS`、
   `pg_get_constraintdef`、`CSIdentifierCacheFetcher_MYSQL` 等；
4. 安装目录随附的官方手册 `navicat.pdf`（385 页）第 17～19 章，逐条给出三个功能的
   选项说明和限制；
5. 对 Docker 中 MySQL 8.4、PostgreSQL 16、SQL Server（Azure SQL Edge）、Oracle
   Free 23 的实测 SQL 语义验证（只验证语句行为，不声称运行过 Navicat UI）。

### 1.3 分析边界

- **观察到**：入口/选项/提示字符串；手册中的选项表与限制；各引擎 catalog 查询涉及
  的表名/函数名；`IDENTITY_INSERT`、`SHOW CREATE TABLE`、`sqlite_master` 等方言标记。
- **推断出**：比较/部署的具体执行顺序、差异模型、事务与批量策略、每引擎的 DDL 生成
  方式。这些由选项语义、手册描述、SQL 语法和对象模型推断，未做函数级反编译。
- **未知**：函数级实现、字符串拼接顺序、内部线程/缓存细节、Lite 版实际开放的功能范围。
- **未做**：驱动 Navicat UI 点击。本文不声称“运行时观察到了 Navicat 的某次执行”。

---

## 2. 三个功能共用的底层模型

### 2.1 对象发现与标识符缓存

`libcc.dll` 中可以直接看到按引擎分开的标识符缓存类（RTTI/符号名）：

```text
CSIdentifierCacheFetcher_MYSQL
CSIdentifierCacheFetcher_MARIADB
CSIdentifierCacheFetcher_SNOWFLAKE
```

其余引擎使用同一族 `CSIdentifierCacheFetcher_*` 命名（MSSQL/ORA/PGSQL/SQLITE
在字符串中未直接暴露完整类名，属推断），并可看到：

```text
fetchFields_information_schema
fetchFunctions_information_schema
fetchTableViews_information_schema
```

这说明 Navicat 不是每次比较都现场拼 SQL，而是先按引擎抓取一份“库/模式/表/字段/
索引/约束/函数/视图”的标识符缓存，再在同一份缓存上做映射和比较。这也解释了为什么
数据同步能先做表/字段映射，再比较：映射发生在对象模型层，不依赖两边的物理名字相同。

### 2.2 比较引擎

三个功能共用“快照 → 归一化 → 差异”的模型：

- 结构：对象（表、视图、函数、序列、触发器……）按名字配对；同名对象内部再按
  字段、索引、主键、唯一键、外键、检查、排除、规则、分区、表选项、字符集、排序规则、
  自增值、identity 最后值、所有者、表空间/物理属性、定义者等维度比较。
- 数据：表按名字配对；表内按“比较键”配对行，再按字段映射比较列值。
- 传输：不比较，直接按“源对象 → 目标对象”搬运；建表时按目标引擎生成 DDL。

### 2.3 部署引擎

比较结果不是直接执行，而是先生成一组部署脚本。部署阶段的通用能力：

- 继续/忽略错误（`Continue on error`）；
- 事务（`Use transaction` / `Use single transaction` / `Begin Transaction` /
  `Commit %d Records` / `Rollback`）；
- 一次执行多条语句（`Run multiple queries in each execution`）；
- 记录执行的 SQL（`Include deployment query in message log`）；
- 调整脚本顺序（`Reorder Script`）；
- 在查询编辑器打开脚本（`Open Script in Query Editor`）；
- 保存为 profile，供下次或自动化任务复用。

这解释了 Navicat 为什么在“结构同步”里也提供“Continue on error”：部署的是脚本列表，
不是一次不可拆的 API 调用。

### 2.4 Profile 与自动化

字符串中可见：

```text
Data Transfer Profile
Data Sync Profile
Structure Sync Profile
Data transfer profile stores settings for future use.
Data sync profile stores settings for future use.
Structure sync profile stores settings for future use.
```

手册第 17～19 章都提到 “Save Profile / Load Profile”，并说明 profile 可用于
自动化任务。三个功能的 profile 扩展名在手册附录中按引擎区分（如 `.ndtfmysql`、
`.ndsymysql`、`.nssymysql`）。

---

## 3. 数据传输（Data Transfer）

### 3.1 入口与流程

手册第 17 章（p233～236）：

1. `Tools → Data Transfer`；
2. 选源连接/库/模式、目标连接/库/模式；目标是文件时选 `File`、SQL 格式与编码；
3. `Options` 设置传输/高级选项；
4. 在 `Database Objects` 里勾选对象，并可为单表设置 `Transfer Mode`；
5. 看摘要；
6. `Start`。

支持源/目标跨服务器类型（手册明确举例 MySQL → Oracle）；源是 MongoDB 时只能传到
MongoDB。支持 MySQL、Oracle、PostgreSQL、SQLite、SQL Server、MariaDB、MongoDB、
Snowflake。

### 3.2 高级选项（手册原文归纳）

| 选项 | 作用 |
|---|---|
| Continue on error | 传输过程中忽略错误继续 |
| Convert object name to | 对象名转小写/大写 |
| Create collections / Create tables | 目标不存在时创建集合/表；不勾选且目标已存在时，数据追加到目标 |
| Create records | 传输记录 |
| Create target database/schema if not exist | 目标库/模式不存在时创建 |
| Drop target objects before create | 传输前删除目标已存在的对象 |
| Drop with CASCADE | 删除时级联依赖对象 |
| Include auto increment | 建表时包含自增 |
| Include character set | 包含字符集 |
| Include checks | 包含检查约束 |
| Include definers | 包含定义者 |
| Include engine/table type | 包含引擎/表类型 |
| Include excludes | 包含排除约束 |
| Include foreign key constraints | 包含外键 |
| Include indexes | 包含索引 |
| Include other table options | 包含其他表选项 |
| Include owners | 包含所有者 |
| Include rules | 包含规则 |
| Include triggers | 包含触发器 |
| Include uniques | 包含唯一键 |
| Lock source tables / Lock target tables | 传输期间锁源/目标表 |
| Maximum statement size | 单条扩展 INSERT 的最大 KB 数，超过则退回普通 INSERT |
| Use complete insert statements | 完整列名 INSERT |
| Use extended insert statements | 多行 VALUES 的扩展 INSERT |
| Use delayed insert statements | `INSERT DELAYED` |
| Use ignore insert statements | `INSERT IGNORE` |
| Use replace insert statements | `REPLACE INTO`（主键/唯一键冲突时先删后插） |
| Use hexadecimal format for BLOB | BLOB 以十六进制写入 |
| Use single transaction | 全程一个事务 |
| Use transaction | 使用事务 |
| Use DDL from SHOW CREATE TABLE | MySQL/MariaDB 用服务器返回的 DDL |
| Use DDL from sqlite_master | SQLite 用 `sqlite_master` 里的 DDL |

这些选项说明 Navicat 的传输不是“导出 CSV 再导入”，而是**按目标引擎生成 DDL + 批量
DML**，并且针对 MySQL 家族暴露了服务器方言的快速路径（`SHOW CREATE TABLE`）和
多种 INSERT 变体。

### 3.3 Transfer Mode（单表/单视图）

手册 p236：

| 选项 | 作用 |
|---|---|
| Target Name | 目标表/视图名（可改名） |
| All fields / Custom fields | 全部字段或挑选字段，并可改目标字段名 |
| All rows | 全部行 |
| Number of rows per batch | 每批行数；不启用则整表作为一个事务发送 |
| Custom recordsets | 用表达式过滤要传输的记录 |
| Recordset Generator | 按某个字段的起止值把大表切成多个 recordset，避免连接超时 |
| SQL Preview | 预览取记录的 SQL |
| Use transaction for each recordset | 每个 recordset 一个事务 |
| Transfer as table | 把视图当作表传输到目标 |

这直接对应“源表很大、连接会超时”的场景：Navicat 用字段区间切分 recordset，而不是
简单 `LIMIT/OFFSET` 翻页。

### 3.4 底层执行顺序（推断）

由选项语义和引擎能力推断，一次传输大致是：

1. 建目标库/模式（如果勾选且不存在）；
2. 按依赖顺序删除目标对象（如果勾选 `Drop target objects before create`）；
3. 按依赖顺序建表：先表结构，再主键/唯一键/索引/外键/检查/触发器；
4. 逐表写数据：
   - 目标表不存在且勾选 `Create tables` → 先建表；
   - 目标表已存在且未勾选 `Create tables` → 直接追加；
   - 按 `Number of rows per batch` / `Maximum statement size` 切批；
   - 按 `Use extended/ignore/replace/delayed insert` 选择 DML 形态；
   - 按 `Use transaction` / `Use single transaction` 决定提交边界；
5. 记录日志；`Continue on error` 决定某条失败后是否继续。

> 观察到的支撑：手册把 `Create tables`、`Create records`、`Include indexes`、
> `Include foreign key constraints`、`Include triggers` 分开列出，说明建表与建索引/
> 外键/触发器是分阶段的，不是一条 `CREATE TABLE` 全包。`IDENTITY_INSERT` 字符串在
> `libnp2.dll` 中出现，说明 SQL Server 传输会显式打开 identity 插入。

---

## 4. 数据同步（Data Synchronization）

### 4.1 入口与流程

手册第 18 章（p237～239）：

1. `Tools → Data Synchronization`；
2. 选源/目标连接、库、模式；
3. `Options` 选比较/高级选项；
4. 选对象，设置源/目标映射和比较条件；
5. `Compare & Preview` 看差异，或 `Compare & Deploy` 直接部署；
6. 预览时逐项勾选要应用的变更，`Next` 生成脚本；
7. 设置部署选项；
8. `Start`。

手册明确：Navicat Premium / Navicat for MySQL 支持 MySQL 与 MariaDB 之间同步。
SQL Server 2000 不支持该功能。

### 4.2 比较选项

手册 p237：

| 选项 | 作用 |
|---|---|
| Insert records | 目标不存在的记录插入 |
| Delete records | 删除目标多余记录 |
| Update records | 记录不同时更新目标 |

字符串表还给出一个更细的选项：

```text
Compare source values with case sensitive
```

即“源值比较是否区分大小写”，默认不区分（跟随引擎/排序规则语义）。这与结构同步的
`Identifier Case Sensitivity` 是两个不同层面的开关：前者比较数据值，后者比较标识符。

### 4.3 键映射、字段映射与条件

手册 p238 是本功能最关键的“底层逻辑”描述：

- 默认只自动映射**同名**的表/集合；不想同步的可以手动禁用；
- 键和字段默认按同名自动映射；结构不同时可以在 `Key Mapping` / `Field Mapping`
  列手工改；
- `Conditions` 列可以给源和目标分别加 SQL 过滤条件，例如
  `id > 10000 AND id < 20000`，限制参与比较的数据范围。

这意味着 Navicat 的数据同步不是“按主键全表比对”一种模式，而是：

```text
源表 --(字段映射)--> 逻辑行
目标表 --(字段映射)--> 逻辑行
两边各自 --(条件)--> 参与比较的行集合
按“键映射”配对 --> Insert / Update / Delete
```

键映射允许源 `id` 对目标 `user_id`，字段映射允许源 `name` 对目标 `full_name`。
这正是 rustgrid 目前缺的最大一块：rustgrid 只按同名列和主键比较。

### 4.4 比较结果视图

手册 p238～239：

- 比较后显示将插入/更新/删除的记录数；
- `Show identical table and others` 可隐藏数据相同、结构不同的表；
- 默认勾选所有有差异的表和操作；
- 选中表后，底部网格左右显示源/目标数据，差异值高亮；
- 右键 `Show Assistant Viewer` 可看多行；
- 可以逐行取消不想应用的记录；
- 视图下拉：`Difference` / `Insert` / `Update` / `Delete` / `Same` / `All Rows`。

这比 rustgrid 的“数据比对”页更细：rustgrid 能按 Insert/Update/Delete 过滤并显示
变更，但不能逐行勾选，也不能看 `Same` / `All Rows`。

### 4.5 部署选项

手册 p239：

| 选项 | 作用 |
|---|---|
| Continue on error | 执行过程中忽略错误继续 |
| Run multiple queries in each execution | 一次执行多条 SQL，加快速度 |
| Use transaction | 出错时回滚全部数据 |
| Include deployment query in message log | 在消息日志中显示执行的 SQL |
| Reorder Script | 调整脚本顺序 |
| Open Script in Query Editor | 在查询编辑器打开脚本 |

### 4.6 比较/部署算法（推断）

结合选项与各引擎能力，Navicat 的数据同步大致是：

1. 读取两边表的字段、主键/唯一键、可空性；
2. 按 `Key Mapping` 决定比较键；没有可用键时，通常要求用户指定，或退回整行比较/
   全量覆盖（具体行为依赖版本与引擎，本文不把未观察到的细节写成结论）；
3. 按 `Field Mapping` 把源列映射到目标列；
4. 两边各自套 `Conditions`；
5. 分页/分批读取，按比较键在内存中配对（或排序后归并）；
6. 生成 Insert/Update/Delete 变更集合；
7. 预览时逐行勾选，生成部署脚本；
8. 部署时按 `Continue on error` / `Use transaction` 执行。

> 观察到的支撑：`Number of records per batch`、`Number of records per fetch`、
> `Number of records per update`、`Fetched Records`、`Processed Records` 等字符串表明
> 数据同步是分批读取/分批写入的，而不是一次性把整表读进内存。

---

## 5. 结构同步（Structure Synchronization）

### 5.1 入口与流程

手册第 19 章（p240～242）：

1. `Tools → Structure Synchronization`；
2. 选源/目标连接、库、模式；
3. `Options` 选比较/高级选项；
4. `Compare` 显示差异；
5. 勾选要同步的对象；
6. `Next` 生成脚本；
7. 设置部署选项；
8. `Start`。

手册明确支持的引擎：MySQL、Oracle、PostgreSQL、SQL Server、MariaDB、MongoDB、
Snowflake。**注意：手册的 Structure Synchronization 列表没有 SQLite**；SQLite 只出现
在 Data Transfer / Data Synchronization / Data Generation 的支持列表里。这与 SQLite
的 `ALTER TABLE` 能力有限、复杂结构变更需要“重建表”有关。

### 5.2 比较选项（完整清单）

手册 p240～241 与字符串表一致：

| 选项 | 比较内容 |
|---|---|
| Compare tables | 表 |
| Compare views | 视图 |
| Compare functions | 函数/存储过程 |
| Compare events | 事件 |
| Compare sequences | 序列 |
| Compare materialized views | 物化视图 |
| Compare triggers | 触发器 |
| Compare indexes | 索引 |
| Compare primary keys | 主键 |
| Compare uniques | 唯一键 |
| Compare foreign keys | 外键 |
| Compare checks | 检查约束 |
| Compare excludes | 排除约束 |
| Compare rules | 规则 |
| Compare partitions | 分区 |
| Compare table options | 其他表选项 |
| Compare storage | 表存储 |
| Compare tablespace and physical attributes | 表空间/物理属性 |
| Compare auto increment value | 自增当前值 |
| Compare identity last value | identity 最后值 |
| Compare sequence last values | 序列最后值 |
| Compare character set | 字符集 |
| Compare collation | 排序规则 |
| Compare owners | 所有者 |
| Compare definers | 定义者 |
| Drop with CASCADE | 删除时级联 |
| Identifier Case Sensitivity | 标识符大小写：忽略/考虑/服务器默认 |

这张表是“查漏补缺”的核心参照：Navicat 的结构同步覆盖了表以外的对象，也覆盖了
自增值、identity/sequence 当前值、所有者、定义者、表空间、存储、分区等“非列定义”
属性；rustgrid 目前只覆盖表内的列/索引/唯一键/检查/外键/排除/规则/分区/表选项。

### 5.3 结果树与 DDL 对比

手册 p241～242：

- 结果按对象类型或操作分组（`Group by object type` / `Group by operation`）；
- 四种操作：
  - 两边都有但定义不同 → 修改；
  - 目标没有 → 创建；
  - 源没有 → 删除；
  - 两边相同 → 不做；
- 选中对象后 `DDL Comparison` 左右并排显示源/目标 DDL，增/删/改高亮；
- `Deployment Script` 显示将要执行的脚本。

这说明结构同步的比较单位是**对象**，对象内部再细化到属性；不是只比较 `SHOW CREATE`
的整段文本。否则无法在“修改”操作里只生成一条 `ALTER TABLE ... MODIFY COLUMN`。

### 5.4 部署选项

手册 p242：

| 选项 | 作用 |
|---|---|
| Continue on error | 忽略错误继续 |
| Include deployment query in message log | 日志显示执行的 SQL |
| Reorder Script | 调整脚本顺序 |
| Open Script in Query Editor | 在查询编辑器打开脚本 |
| Drop with CASCADE | 删除时级联 |

### 5.5 结构比较/部署算法（推断）

1. 按引擎 catalog 读取对象定义，归一化为对象模型；
2. 按对象名配对（受 `Identifier Case Sensitivity` 影响）；
3. 对同名对象逐属性比较；对只在一侧的对象生成 Create/Drop；
4. 对“修改”生成 `ALTER`；对无法 `ALTER` 的变更（SQLite 的复杂变更、SQL Server 的
   identity、列顺序等）退回“重建表/重建对象”；
5. 生成部署脚本；
6. 按依赖顺序执行（表 → 索引/约束/触发器；视图/函数/序列按依赖或按脚本顺序）；
7. `Continue on error` / `Reorder Script` 控制执行。

> 观察到的支撑：手册把 `Compare indexes`、`Compare foreign keys`、`Compare triggers`
> 等拆成独立开关，说明这些对象在模型里是独立节点，部署时可以单独生成脚本；
> `Reorder Script` 的存在说明脚本顺序不是固定的，用户可以干预。

---

## 6. 分引擎矩阵

> 标记说明：**观察** = 手册/字符串/二进制中直接可见；**推断** = 由引擎语义和选项推断。

### 6.1 MySQL

| 维度 | 内容 |
|---|---|
| 支持的功能 | 传输、数据同步、结构同步；数据同步支持 MySQL ↔ MariaDB（**观察**） |
| 对象发现 | `information_schema`：`COLUMNS`、`STATISTICS`、`KEY_COLUMN_USAGE`、`REFERENTIAL_CONSTRAINTS`、`TABLES`、`TRIGGERS`、`ROUTINES`、`EVENTS`、`PARTITIONS`、`CHECK_CONSTRAINTS`（**推断 + libcc 中 information_schema 查询串**） |
| DDL 来源 | 可选 `Use DDL from SHOW CREATE TABLE`（**观察**） |
| 结构比较 | 表、视图、函数、事件、触发器、索引、主键、唯一键、外键、检查、分区、表选项、字符集、排序规则、自增值、定义者（**观察 + 推断**） |
| 数据传输 DML | 完整/扩展/`IGNORE`/`REPLACE`/`DELAYED` INSERT；十六进制 BLOB；单事务/分批提交；锁源/目标表（**观察**） |
| 特殊点 | `AUTO_INCREMENT` 是表选项；生成列（`GENERATED ... VIRTUAL/STORED`）不能显式写入；`utf8mb4` 字符集/排序规则是列/表级属性；分区是 `PARTITION BY` 子句；`ON UPDATE CURRENT_TIMESTAMP` 在 `EXTRA` 里（**推断**） |
| 限制 | 结构同步对 `ALTER` 不支持的变更会重建表；`DELAYED` 在新版本 MySQL 已移除，但 Navicat 仍保留该选项（**观察 + 推断**） |

### 6.2 MariaDB

| 维度 | 内容 |
|---|---|
| 支持的功能 | 同 MySQL；手册明确 MySQL ↔ MariaDB 同步（**观察**） |
| 与 MySQL 的差异 | 序列（10.3+）、系统版本表、`CHECK` 约束、`JSON` 别名、`UUID`/`INET6` 类型、向量索引（Navicat 18 设计器有 `VECTOR INDEX` 的 `Distance Function`/`M`）（**观察 + 推断**） |
| 结构比较 | 在 MySQL 基础上增加序列；其余同 MySQL（**推断**） |
| DDL 来源 | `SHOW CREATE TABLE`（**观察**） |
| 特殊点 | MariaDB 的 `SHOW CREATE TABLE` 带版本注释；系统版本表有 `WITH SYSTEM VERSIONING`；序列是独立对象（**推断**） |

### 6.3 SQL Server

| 维度 | 内容 |
|---|---|
| 支持的功能 | 传输、数据同步、结构同步；SQL Server 2000 不支持数据同步（**观察**） |
| 对象发现 | `sys.tables`、`sys.columns`、`sys.types`、`sys.indexes`、`sys.index_columns`、`sys.foreign_keys`、`sys.check_constraints`、`sys.computed_columns`、`sys.identity_columns`、`sys.triggers`、`sys.objects`、`sys.sql_modules`、`sys.schemas`、`sys.extended_properties`（**推断 + 二进制中的 sys 查询串**） |
| 结构比较 | 表、视图、函数/过程、触发器、索引、主键、唯一键、外键、检查、所有者（schema）、identity 最后值、存储（文件组/压缩）、序列（2012+）（**观察 + 推断**） |
| 数据传输 DML | 参数化 INSERT；`SET IDENTITY_INSERT ... ON/OFF`（`libnp2.dll` 中直接出现 `IDENTITY_INSERT`，**观察**）；`N'...'` Unicode 字面量；`[方括号]` 标识符（**推断**） |
| 特殊点 | identity 列必须 `SET IDENTITY_INSERT` 才能显式插入；`ALTER COLUMN` 不能新增 identity；列顺序不可改；注释用扩展属性；计算列不能写入；`uniqueidentifier`/`datetime2`/`varbinary(max)`/`money` 等类型需要目标方言映射（**推断**） |
| 限制 | 一个会话同时只能有一张表 `IDENTITY_INSERT ON`；`SET IDENTITY_INSERT` 通过 `sp_executesql` 发送时只在该嵌套批次内有效，必须用普通 batch 或与 INSERT 同批（**实测确认**） |

### 6.4 Oracle

| 维度 | 内容 |
|---|---|
| 支持的功能 | 传输、数据同步、结构同步（**观察**） |
| 对象发现 | `ALL_TAB_COLUMNS`、`ALL_TABLES`、`ALL_CONSTRAINTS`、`ALL_CONS_COLUMNS`、`ALL_INDEXES`、`ALL_IND_COLUMNS`、`ALL_TAB_PARTITIONS`、`ALL_VIEWS`、`ALL_MVIEWS`、`ALL_SEQUENCES`、`ALL_TRIGGERS`、`ALL_SOURCE`、`ALL_TAB_COMMENTS`、`ALL_COL_COMMENTS`、`ALL_TAB_IDENTITY_COLS`（**推断 + 二进制中 `"SYS"."ALL_TAB_COLUMNS"` 等串**） |
| 结构比较 | 表、视图、物化视图、函数/过程/包、序列、触发器、索引、主键、唯一键、外键、检查、所有者、表空间/物理属性、分区、identity 最后值、定义者（**观察 + 推断**） |
| 数据传输 DML | 普通 INSERT/批量；`TRUNCATE TABLE` 清空；`DROP TABLE ... CASCADE CONSTRAINTS`；LOB 需要特殊处理（**推断**） |
| 特殊点 | 无 `SHOW CREATE TABLE`，DDL 由 catalog + `DBMS_METADATA.GET_DDL` 重建；未加引号的标识符大写、加引号区分大小写；`GENERATED ALWAYS/BY DEFAULT AS IDENTITY`；`ALTER SESSION SET CURRENT_SCHEMA`；表空间/存储/分区是重要比较项（**推断 + 手册/二进制**） |
| 限制 | **数据同步会跳过 BLOB、CLOB、NCLOB、LONG、LONG RAW**（手册 p237，**观察**）；TIMESTAMP 主键在 Database Link 到 9i 时不能 insert/update；RAW 主键在 Database Link 到任何服务器时不能 insert/update/delete 且不报错（手册 p237，**观察**） |

### 6.5 PostgreSQL

| 维度 | 内容 |
|---|---|
| 支持的功能 | 传输、数据同步、结构同步（**观察**） |
| 对象发现 | `pg_catalog` + `information_schema`：`pg_class`、`pg_attribute`、`pg_index`、`pg_constraint`、`pg_attrdef`、`pg_type`、`pg_namespace`、`pg_trigger`、`pg_rewrite`、`pg_sequence`、`pg_tablespace`、`pg_authid`；`pg_get_constraintdef`、`pg_get_indexdef`、`pg_get_triggerdef`、`pg_get_ruledef`、`pg_get_viewdef`、`pg_get_functiondef`、`format_type`（**推断 + libcc 中 pg_catalog 查询串**） |
| 结构比较 | 表、视图、物化视图、函数、序列、触发器、索引、主键、唯一键、外键、检查、排除约束、规则、所有者、表空间/存储、分区、定义者、identity 最后值、序列最后值（**观察 + 推断**） |
| 数据传输 DML | 参数化 INSERT；identity `GENERATED ALWAYS` 需要 `OVERRIDING SYSTEM VALUE`；`bytea` 十六进制；`jsonb`/数组/`numeric` 需文本化或类型转换；批量可用 `COPY`（**推断**） |
| 特殊点 | 对象名 schema 限定（`public.users`）；`search_path`；identity vs `serial`；`GENERATED ALWAYS AS IDENTITY` 拒绝显式值；`ALTER TABLE` 不能改列顺序；排除约束/规则是 PostgreSQL 特有比较项；owner/definer 是角色（**推断**） |
| 限制 | 跨库不能查询，同步工具需要按数据库分别连接；`numeric`/`uuid`/`json`/数组在二进制协议下需要 `::text` 投影（rustgrid 驱动已处理，**推断**） |

### 6.6 SQLite

| 维度 | 内容 |
|---|---|
| 支持的功能 | 传输、数据同步（手册支持列表）；**结构同步手册列表不含 SQLite**（**观察**） |
| 对象发现 | `sqlite_master`、`PRAGMA table_info`、`PRAGMA index_list`、`PRAGMA index_info`、`PRAGMA foreign_key_list`、`PRAGMA table_xinfo`、`sqlite_sequence`（**推断 + 二进制中 `PRAGMA STATEMENT` / `sqlite_master` 串**） |
| DDL 来源 | 可选 `Use DDL from sqlite_master`（**观察**） |
| 结构比较 | 表、视图、触发器、索引、主键、唯一键、外键、检查（SQLite 3.3+ 支持部分 `ALTER TABLE`）（**推断**） |
| 数据传输 DML | 普通 INSERT、事务；`INTEGER PRIMARY KEY AUTOINCREMENT`；无 identity/owner/tablespace/sequence（**推断**） |
| 特殊点 | 类型亲和（type affinity），不是强类型；日期/时间存文本；`STRICT` 表（3.37+）；生成列（3.31+）；复杂结构变更需要“建新表 → 复制 → 删旧表 → 改名”，并在事务中关闭/恢复 `PRAGMA foreign_keys`（**推断**） |
| 限制 | 手册的 Structure Synchronization 不支持 SQLite；若要做结构同步，必须实现重建表，不能只靠 `ALTER TABLE`（**观察 + 推断**） |

### 6.7 汇总矩阵

| 能力 | MySQL | MariaDB | SQL Server | Oracle | PostgreSQL | SQLite |
|---|---|---|---|---|---|---|
| 数据传输 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 数据同步 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 结构同步 | ✅ | ✅ | ✅ | ✅ | ✅ | 手册未列 |
| MySQL↔MariaDB 同步 | ✅ | ✅ | — | — | — | — |
| `SHOW CREATE` DDL 快速路径 | ✅ | ✅ | — | — | — | `sqlite_master` |
| identity 显式插入 | 自增可显式 | 自增可显式 | `IDENTITY_INSERT` | `BY DEFAULT` 可，`ALWAYS` 不可 | `OVERRIDING SYSTEM VALUE` | `INTEGER PRIMARY KEY` 可 |
| 序列对象 | — | ✅ | ✅(2012+) | ✅ | ✅ | — |
| 物化视图 | — | — | — | ✅ | ✅ | — |
| 排除约束/规则 | — | — | — | — | ✅ | — |
| 表空间/存储/物理属性 | 表空间（InnoDB） | 表空间 | 文件组/压缩 | ✅ | ✅ | — |
| 分区 | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| LOB 同步 | ✅ | ✅ | ✅ | 手册：跳过 LOB | ✅ | ✅ |

---

## 7. rustgrid 现状

### 7.1 已有能力

rustgrid 的同步工作台（`crates/rustgrid-app/src/app/sync.rs` + `crates/rustgrid-core/src/sync.rs`）
目前是“一个窗口、四个模式”：

- 模式：`全部`、`仅结构`、`仅数据`、`全量传输`；
- 源/目标各自选连接 + 数据库；
- 表列表 + 每表状态（New / Same / Structure(n) / n row diff / No Key / Overwrite / Synced）；
- 检查器三个页签：结构比对、数据比对、SQL 预览；
- 选项：包含 DELETE、只同步目标已存在的表、隐藏一致表；数字键表全量流式比较，
  文本键表保留每表 5000 行比较上限；
- 运行：逐表比较、逐表应用、日志、取消、结束后重比较。

结构比较 `diff_schema` 已覆盖：

- 列：类型/长度/小数位/unsigned/可为空/主键/自增/默认值/生成列/stored/charset/collation/
  comment/extra/zerofill；
- 索引（非主键）、唯一键、检查、外键、排除约束、规则、分区、表选项
  （engine/charset/collation/comment；自增计数器刻意忽略）。

数据比较/应用：

- 按主键（本次新增唯一键回退）配对；
- 逐列按显示值比较；
- Insert/Update/Delete；
- 批量 500 行写目标；
- `全量传输`/每表 `全量覆写` 先清空再复制；
- 结构应用复用目标驱动的 `save_table_schema`，数据应用复用 `insert_rows`/
  `update_rows`/`delete_rows`，不在 UI 里拼 SQL。

### 7.2 差距清单

> 优先级：P0 = 影响正确性/基本可用；P1 = 影响完整度/大表/跨引擎；P2 = 体验/自动化。

| # | 差距 | Navicat 的做法 | rustgrid 原状 | 本次处理 |
|---|---|---|---|---|
| 1 | 仅数据模式是否应给缺失表建表 | `Create tables` 是独立选项，不由数据同步模式隐式决定 | 旧实现让仅数据也建表，模式边界不清 | **已明确**：仅数据只写已存在的表；`全部`/`仅结构`/`全量传输`负责创建缺失表（跨引擎同样如此） |
| 2 | 无主键表不能增量同步 | 要求选择比较键，或按可用唯一键/整行处理 | 只认 `ColumnInfo.primary_key`，无键就只能全量覆写 | **已修**：`usable_key_columns` 回退到非空唯一键 |
| 3 | 建表顺序不按外键依赖 | 结构脚本按依赖顺序部署，并可 `Reorder Script` | 按表列表顺序建表，子表先建会因外键失败 | **已修**：`order_plans_for_creation` 按 FK 拓扑排序 |
| 4 | 生成列被当作普通列写入 | 生成列由目标计算；比较/写入时排除 | `diff_rows` 把所有列纳入，写入生成列会报错 | **已修**：`prune_generated_changes` + `skip_write` 过滤 |
| 5 | PostgreSQL identity ALWAYS 写入失败 | 保留源 identity 值 | 普通 INSERT 被 PostgreSQL 拒绝 | **已修**：驱动对 ALWAYS identity 自动加 `OVERRIDING SYSTEM VALUE` |
| 6 | SQL Server identity 写入失败 | `SET IDENTITY_INSERT` | 普通 INSERT 被 SQL Server 拒绝 | **已修**：`insert_rows` 在 identity 表上打开/关闭 `IDENTITY_INSERT` |
| 7 | 大表只比较前 5000 行 | 分批读取/分批写入，无固定行数上限 | `SYNC_MAX_ROWS = 5000`，超出显示“不完整” | **已修（数字键）**：键有序流式归并，无行数上限；文本键因排序规则仍走有界比较（见 8.8） |
| 8 | 跨引擎不能建表/同步结构 | 跨服务器类型传输，按目标引擎映射类型 | 跨引擎结构被锁死，数据只能写入已存在的目标表 | **已修（缺失表）**：逻辑类型映射 + 目标方言 DDL；已有表不做跨方言结构 ALTER（见 8.9） |
| 9 | Oracle identity ALWAYS / LOB 策略 | 手册：同步跳过 BLOB/CLOB/NCLOB/LONG/LONG RAW；identity 按 `GENERATED` 处理 | Oracle catalog 只读 `identity_column`，不读 `GENERATION_TYPE`；LOB 照常读 | **部分修复**：读 `ALL_TAB_IDENTITY_COLS`，`ALWAYS` 不写值；LOB 不参与比较/写入，但 `fetch_page` 仍会读回 LOB（见 8.10） |
| 10 | SQLite 生成列/复杂结构变更 | 结构同步手册未列 SQLite；若做，需重建表 | SQLite `table_schema` 不解析生成列；复杂 ALTER 依赖驱动 | 未修（P1） |
| 11 | 表名大小写匹配 | `Identifier Case Sensitivity` 可配置 | 目标表名 `contains` 大小写敏感 | **部分修复**：`resolve_target_name` 大小写不敏感并支持 `public.users`/`users` 互认；仍未暴露“忽略/考虑/服务器默认”选项（P1） |
| 12 | 键/字段映射、目标表改名 | `Key Mapping` / `Field Mapping` / `Target Name` | 只按同名表、同名列 | 未修（P1，需 UI 才能配置；本次保持界面不变） |
| 13 | 比较条件/recordset | `Conditions` / `Custom recordsets` / `Recordset Generator` | 无 | 未修（P1） |
| 14 | 逐行勾选变更 | 预览网格可逐行取消 | 只能按 Insert/Update/Delete 过滤显示，不能取消单行 | 未修（P1） |
| 15 | 部署脚本/顺序/在编辑器打开 | `Deployment Script` / `Reorder Script` / `Open Script in Query Editor` | SQL 预览只读，不能导出/排序 | 未修（P1） |
| 16 | Continue on error / 事务 / 批量大小 | 部署选项齐全 | 表级失败继续下一表；表内失败即中止；批量固定 500 | 未修（P1） |
| 17 | 视图/函数/序列/触发器/owner/definer/表空间/存储/分区 | 结构同步全套对象 | 只同步表；`TableSchema.triggers` 未参与 diff/apply | 未修（P1/P2，受“界面不变”限制） |
| 18 | 自增值/identity 最后值/sequence 最后值 | 独立比较开关 | 刻意忽略 `AUTO_INCREMENT` 计数器 | 未修（P2；忽略计数器是合理的，但 identity/sequence 当前值可另做选项） |
| 19 | 值比较大小写敏感 | `Compare source values with case sensitive` | 固定按显示值精确比较（区分大小写） | 未修（P2） |
| 20 | 传输 DML 变体/锁表/最大语句 | 扩展/IGNORE/REPLACE/DELAYED/锁表/最大 KB | 固定批量 INSERT，无锁表/变体 | 未修（P2） |
| 21 | Profile/自动化 | 三个功能都有 profile 和自动化 | 传输配置已保存（`transfers.json`），无调度 | 未修（P2） |
| 22 | 进度 | 表级 + 记录级（`Processed Records`） | 表级进度条 + 日志 | 未修（P2） |

---

## 8. 本次已补齐的实现

### 8.1 唯一键回退

`rustgrid-core/src/sync.rs` 新增：

```rust
pub fn usable_key_columns(columns: &[ColumnInfo], schema: Option<&TableSchema>) -> Vec<String>
```

规则：

1. 有主键 → 用主键；
2. 否则从 `TableSchema.indexes`（`unique && !primary`）和 `unique_keys` 中找候选；
3. 候选列必须全部存在且**全部 NOT NULL**（唯一键允许 NULL 时不能唯一标识行）；
4. 取列数最少的候选；没有则返回空。

对应测试：`key_columns_fall_back_to_a_non_null_unique_key`、
`key_columns_prefer_the_primary_key`。

### 8.2 模式边界：仅数据不建表

`sync.rs`：

- `SyncTable::has_changes` 中，`仅数据` 要求目标表已经存在；缺失表不算数据变化；
- 缺失表在 `仅数据` 下仍显示“目标不存在”，但不会进入执行计划，运行按钮不会把它算进去；
- 建表只属于 `全部`、`仅结构`、`全量传输`：结构阶段按目标方言创建，随后数据阶段复制；
- `run_sync_table` 的数据阶段不再兜底建表；目标表在比较后被删除时直接报“请先同步结构”；
- SQL 预览在 `仅数据` + 缺失表时提示“不会创建表”，不再显示 `CREATE TABLE`。

对应测试：`data_only_never_plans_a_missing_table`。

### 8.3 外键拓扑排序

`order_plans_for_creation`：

- 从每个待建表的 `source_schema.foreign_keys` 找被引用表；
- 被引用表也在本次创建集合里时，建立依赖边；
- 按依赖顺序输出；环或引用未选表时保留原顺序，不丢表。

对应测试：`creation_order_follows_foreign_keys`。

### 8.4 生成列处理

- `prune_generated_changes`：从数据差异中删除生成列的 update 项；只剩生成列的 update 整体丢弃；
- `skip_write`：写入/全量复制时过滤生成列；
- 这样比较页、运行日志和实际写入一致，不会出现“比较有差异但写入失败/未写入”。

### 8.5 PostgreSQL identity

`rustgrid-postgresql/src/connection.rs`：

- `always_identity_columns` 查 `information_schema.columns` 的
  `is_identity='YES' AND identity_generation='ALWAYS'`；
- `insert_rows` 在该表的插入列包含 ALWAYS identity 时生成
  `INSERT INTO ... (...) OVERRIDING SYSTEM VALUE VALUES (...)`，保留源 id。

实测：`INSERT INTO itest (id, v) OVERRIDING SYSTEM VALUE VALUES (100, 'a')` 在
PostgreSQL 16 上通过。

### 8.6 SQL Server identity

`rustgrid-sqlserver/src/connection.rs::insert_rows`：

- 新增 `identity_column` 查出 identity 列名（`sys.columns.is_identity = 1`）；
- 只有本批 INSERT **确实带了 identity 列**时才打开 `IDENTITY_INSERT`（打开后 SQL Server
  要求每条 INSERT 都显式给出 identity 值，不能对自动生成的批次无条件打开）；
- 通过**普通 batch**（`run_plain_batch`）执行 `SET IDENTITY_INSERT ... ON`，
  再走参数化 INSERT，最后关闭；
- 失败路径也会尝试关闭，避免连接被留在打开状态。

实测：`SET IDENTITY_INSERT` 通过普通 batch 执行后，对后续 `sp_executesql` 的 INSERT
仍然有效；反过来，`SET` 本身走 `sp_executesql` 则不会留下设置。这就是为什么必须在
普通 batch 里发 `SET`。

### 8.7 未改界面

本次没有增加/删除/重排任何控件，也没有改表列表、检查器页签或状态栏结构；
所有新行为都在现有四个模式和现有按钮下生效。只调整了两处说明文案，避免与行为矛盾：

- `sync.hetero`：改成“仅数据只写已存在的表；全部、仅结构、全量传输会按目标引擎类型创建缺失表”；
- `sync.options.limit`：说明数字键表全量流式比较，文本键表仍受 `%{rows}` 行上限。

### 8.8 大表流式比较（数字键）

`sync.rs` 新增 `RowStream` + `compare_rows_streaming`：

- 源/目标各自用 `ORDER BY <key>` 分页读取，页大小 `SYNC_STREAM_PAGE = 2000`；
- `PageRequest::skip_total` 让流式分页跳过每页的 `COUNT(*)`（普通表格分页仍保留总数）；
- 两边游标按键比较，源小 → Insert，目标小 → Delete，相等 → 逐列比较后决定是否 Update；
- 只保留差异，不把整表读进内存，因此数字主键表不再有 5000 行硬上限；
- 文本/二进制键仍使用原来的有界哈希比较：它们的 SQL 排序依赖服务器 collation，
  当前 `PageRequest` 不暴露 collation，贸然归并会在 `A`/`a`、重音字符等场景配错行。
- 差异本身仍保存在内存（`DataDiff.rows`）；内存占用与**差异数**成正比，而不是与表大小
  成正比。全表 1000 万行但只有 10 行不同，内存里也只有 10 条 `RowChange`。

`compare_key_rows` / `compare_key_cells` 对 `Int`/`Uint`/`Float` 做数值比较，
不会把 `10` 排在 `9` 前面。单测覆盖了数值键和“文本键不走流式”的判断。

### 8.9 跨引擎逻辑类型映射

新增 `rustgrid-core/src/portable.rs`：

```rust
pub enum LogicalType { Bool, Int(u8), UInt(u8), Decimal{..}, Float(bool), Text(Option<u32>),
                       Binary(Option<u32>), Date, Time, DateTime{..}, Uuid, Json, Xml, Other(String) }
pub fn map_schema(schema, source: DriverDialect, target: DriverDialect) -> TableSchema
pub fn map_table_name(name, target: DriverDialect) -> String
```

- 每个源列先 `classify` 成逻辑类型，再按目标方言 `render` 成目标原生类型；
- 目标类型示例：
  - MySQL：`tinyint/smallint/int/bigint/decimal/double/varchar/longtext/varbinary/longblob/
    date/time/datetime/timestamp/json`；
  - PostgreSQL：`smallint/integer/bigint/numeric/real/double precision/varchar/text/bytea/
    date/time/timestamp/timestamptz/uuid/jsonb/xml`；
  - SQL Server：`tinyint/smallint/int/bigint/decimal/real/float/nvarchar(n|max)/
    varbinary(n|max)/date/time/datetime2/datetimeoffset/uniqueidentifier/xml`；
  - Oracle：`NUMBER(p[,s])/BINARY_FLOAT/BINARY_DOUBLE/VARCHAR2(n|CLOB)/RAW(n|BLOB)/
    DATE/TIMESTAMP[ WITH TIME ZONE]/CLOB/XMLTYPE`；
  - SQLite：`INTEGER/NUMERIC/REAL/TEXT/BLOB`。
- identity 映射：MySQL `AUTO_INCREMENT` → PG `GENERATED BY DEFAULT AS IDENTITY` /
  SQL Server `IDENTITY(1,1)` / Oracle `GENERATED BY DEFAULT AS IDENTITY` /
  SQLite `INTEGER PRIMARY KEY` 上的 `AUTOINCREMENT`。
- 跨方言无法安全移植的对象被清空：生成列表达式、check、exclude、rule、partition、
  trigger；外键/唯一键/普通索引保留。
- 目标名：schema-less 目标（MySQL/SQLite）用裸表名，schema-aware 目标
  （PostgreSQL/SQL Server/Oracle）保留限定名；`resolve_target_name` 还能把
  `users` 对到目标实际的 `public.users` / `dbo.users`。
- 跨引擎只对**缺失表**执行 `CREATE`，并且只在 `全部`/`仅结构`/`全量传输` 下发生；
  `仅数据` 只写已存在的表。已有表不做跨方言 `ALTER`（类型系统无法逐属性比较），
  只走数据同步/全量覆盖。结构比较时也不再报告跨方言的“结构差异”。

单测覆盖 MySQL→PostgreSQL 的 `bigint/varchar/tinyint/decimal/datetime/blob`、
identity 到各目标、以及 check/partition/trigger 的清除。

### 8.10 Oracle LOB / identity ALWAYS

- `rustgrid-oracle/src/schema.rs` 的 `table_schema` 现在 `LEFT JOIN ALL_TAB_IDENTITY_COLS`，
  读 `GENERATION_TYPE` 并写入 `ColumnDef.identity`；
- `column_sql` 按 `identity` 输出 `GENERATED ALWAYS` 或 `GENERATED BY DEFAULT`，
  `ALTER` 路径同样保留 `ALWAYS`；
- 同步运行时，源是 Oracle 且列类型为 `BLOB/CLOB/NCLOB/LONG/LONG RAW` 时：
  - 比较阶段 `prune_skipped_changes` 把这些列从差异中移除；
  - 写入阶段 `skip_write` 不再写这些列（目标保持默认/NULL），与 Navicat 手册的
    “Oracle 数据同步跳过 LOB”语义一致；
  - 注意：`fetch_page` 目前仍会读回 LOB 列（驱动层没有列投影参数），只是不比较/不写；
    真正省掉 LOB 读取还需要给 `PageRequest` 增加投影，见 9.1。
- `GENERATED ALWAYS` 的 identity 列同样不写值，避免 Oracle 的
  `ORA-32795: cannot insert into a generated always identity column`。

---

## 9. 仍存差距与建议

### 9.1 P0：下一步最值得做的事

1. **文本/二进制键的流式比较。**
   - 让 `PageRequest` 能指定比较用的 collation（MySQL `BINARY`、PostgreSQL `COLLATE "C"`、
     SQL Server `Latin1_General_BIN2`、Oracle `NLSSORT(...BINARY)`、SQLite `BINARY`），
     两边一致后即可把归并扩展到文本/二进制键。
   - 在此之前，文本键表仍受 5000 行有界比较限制。
   - 另需支持 keyset 分页（`WHERE (k1,k2) > (?,?)`），避免深 `OFFSET` 在大表上变慢。

2. **跨引擎已有表的结构同步。**
   - 目前只创建缺失表；已有表跨方言不做 `ALTER`。
   - 若要做，需要把“逻辑类型 + 属性”转换成目标 `ALTER`，并处理：
     `ALTER COLUMN TYPE` 的隐式转换、默认值/排序规则/identity、外键依赖顺序、
     以及 SQLite 的重建表。
   - 建议先做“生成迁移脚本供用户确认”，而不是自动执行。

3. **把类型映射暴露到 UI（不改当前布局的前提下）。**
   - 在传输/同步窗口的现有“选项”弹层里增加“目标类型预览”或“结构脚本”入口；
   - 让用户能看到 `bigint → NUMBER(19)`、`datetime → TIMESTAMP` 这类映射，
     并手工覆盖个别列的映射。
   - 当前映射是自动的、不可编辑的。

4. **给 `PageRequest` 增加列投影，真正跳过 Oracle LOB 读取。**
   - 现在 Oracle LOB 只是不参与比较/写入，`fetch_page` 仍会读回；
   - 加一个 `exclude_columns`/`projection` 字段，让 Oracle 同步页不 SELECT LOB 列，
     同时让 `TablePage.columns` 与返回行保持一致；
   - 顺带可用于“只同步选中字段”的传输模式。

### 9.2 P1：需要 UI 才能配置、但底层可以先做

- 在 `rustgrid-core::sync` 增加 `TableMapping` / `ColumnMapping` / `CompareCondition` 模型，
  先支持“程序内指定”，UI 后续再加；
- 数据同步结果增加 `Same` / `All Rows` 数据源，支持逐行 `selected` 标记；
- 部署脚本增加 `Vec<DeployStatement>`（SQL + 对象 + 顺序 + 是否可跳过），
  支持导出 `.sql` 和在查询编辑器打开；
- 结构同步增加 `CompareOptions` 结构体，先覆盖现有 `diff_schema` 的开关，
  再逐步加入 triggers/views/functions/sequences/owners/tablespaces；
- `CellValue::Bytes` 目前通过 `as_edit_value` 变成十六进制字符串，跨引擎写
  `bytea`/`varbinary`/`BLOB` 时可能被当成文本而不是字节；需要给 `RowInsert` 增加
  字节参数（或目标方言的十六进制字面量）才能完整搬运二进制列。

### 9.3 P2：体验/自动化

- `Continue on error` / `Use transaction` / `Number of rows per batch` 做成运行参数
  （即使 UI 暂不暴露，也可以从配置文件读默认值）；
- `Compare source values with case sensitive`；
- `Lock source/target tables`；
- `Maximum statement size` / 扩展 INSERT；
- Profile 调度（cron/批处理）；
- 记录级进度（`Processed Records`）。

---

## 10. 验证方法（Docker）

本机 Docker 中已有：

```text
db-mysql       mysql:8.4                 127.0.0.1:3306
db-postgres    postgres:16-alpine         127.0.0.1:5432
db-sqlserver   azure-sql-edge             127.0.0.1:1433
db-oracle      gvenzl/oracle-free:23     127.0.0.1:1521
```

可用环境变量跑 rustgrid 的 live 测试：

```powershell
$env:RUSTGRID_MYSQL_PASSWORD='Root123!'
cargo test -p rustgrid-mysql -- --ignored

$env:RUSTGRID_PG_PASSWORD='Root123!'
cargo test -p rustgrid-postgresql -- --ignored

$env:RUSTGRID_SQLSERVER_PASSWORD='Root123!'
cargo test -p rustgrid-sqlserver -- --ignored

$env:RUSTGRID_ORACLE_PASSWORD='Root123!'
cargo test -p rustgrid-oracle -- --ignored
```

本次验证结果：

- PostgreSQL：`round_trips_a_live_postgresql` 通过；`OVERRIDING SYSTEM VALUE` 语法在
  PostgreSQL 16 上单独实测通过。
- SQL Server：`round_trips_a_live_sql_server` 通过；测试中新增了一条显式 `id=100` 的
  identity 插入，验证 `IDENTITY_INSERT` 包装有效。
- MySQL：`round_trips_a_live_*` 全部通过；新增的
  `live_schema_keeps_an_explicit_empty_string_default` 与
  `live_table_statuses_report_exact_row_counts` 通过（`DEFAULT ''` 不再与 `DEFAULT NULL`
  合并，列表行数与 `COUNT(*)` 一致）。仅
  `live_auth_failure_maps_to_authentication` 因 `sqlx` 的 `mysql-rsa` 后端未启用而失败，
  与本次同步改动无关。
- Oracle：`live_oracle` 的 4 个用例全部通过（连接、浏览、编辑、设计器往返），
  证明 `ALL_TAB_IDENTITY_COLS` 查询与 `GENERATED ALWAYS` DDL 没有破坏既有流程。
- 核心单测：`rustgrid-core` 25 个通过（含唯一键、生成列、流式键比较、类型映射）。
- 应用单测：`RustGrid` 163 个通过，2 个忽略；新增的流式/生成列/外键排序/类型映射/
  目标名解析/仅数据模式边界相关测试均通过。
- SQLite：本次未跑 live 测试；MySQL→SQLite 的映射 DDL 有单测覆盖。
- 跨引擎 DDL 单测：MySQL→PostgreSQL / SQL Server / Oracle / SQLite、PostgreSQL→MySQL
  各自的 `CREATE TABLE` 生成均已通过。

> 注意：MySQL 8.4 的 `live_auth_failure_maps_to_authentication` 失败是
> `sqlx` 的 `mysql-rsa` 特性/认证后端问题，不是同步改动引起的。

---

## 附录 A：关键字符串证据

以下字符串来自 `navicat.exe` 的 UTF-16 字符串表（除注明外）：

```text
Data Transfer
Data Synchronization
Structure Synchronization
Data Transfer Profile
Data Sync Profile
Structure Sync Profile
Data transfer profile stores settings for future use.
Data sync profile stores settings for future use.
Structure sync profile stores settings for future use.

Continue on error
Use transaction
Use single transaction
Auto begin transaction
Begin Transaction
Commit
Commit %d Records
Commit 1 Record
Rollback
Batch Size
Number of records per batch
Number of records per fetch
Number of records per update
Lock tables for write

Use DDL from SHOW CREATE TABLE
Use DDL from sqlite_master
Use complete insert statements
Use extended insert statements
Use delayed insert statements
Use ignore insert statements
Use replace insert statements
Use hexadecimal format for BLOB
Maximum statement size

Create tables
Create records
Create indexes
Create primary keys and indexes
Create triggers
Drop target objects before create
Drop with CASCADE
Include auto increment
Include character set
Include checks
Include definers
Include engine/table type
Include excludes
Include foreign key constraints
Include indexes
Include other table options
Include owners
Include rules
Include triggers
Include uniques

Insert records
Delete records
Update records
Compare source values with case sensitive
Identifier Case Sensitivity

Compare auto increment value
Compare character set
Compare checks
Compare collation
Compare definers
Compare events
Compare excludes
Compare foreign keys
Compare functions
Compare identity last value
Compare indexes
Compare materialized views
Compare owners
Compare partitions
Compare primary keys
Compare rules
Compare sequence last values
Compare sequences
Compare storage
Compare table options
Compare tables
Compare tablespace and physical attributes
Compare triggers
Compare uniques
Compare views

Copy: delete all records in destination, repopulate from the source
Delete: delete records in destination that match records in source
Update: update records in the destination with matching records from source
Append without update: if records exist in destination, skip it. Otherwise, add it
Append/Update: if records exist in destination, update it. Otherwise, add it
Empty target tables before creating records

Show synchronization detail
Comparison completed
Comparison failed
Comparison cancelled
Do you want to continue with the Structure Synchronize?
Do you want to stop the transfer process?
Do you want to stop the synchronous process?
```

来自 `libnp2.dll` 的 ASCII 字符串：

```text
IDENTITY_INSERT
PRAGMA
SHOW CREATE TABLE STATEMENT
DROP TABLE STATEMENT
TRUNCATE TABLE
ALTER SESSION
```

来自 `libcc.dll` 的 ASCII 字符串（catalog/对象发现）：

```text
CSIdentifierCacheFetcher_MYSQL
CSIdentifierCacheFetcher_MARIADB
CSIdentifierCacheFetcher_SNOWFLAKE
fetchFields_information_schema
fetchFunctions_information_schema
fetchTableViews_information_schema
"SYS"."ALL_TAB_COLUMNS"
FROM information_schema.columns AS col LEFT JOIN pg_namespace ...
```

---

## 附录 B：手册章节

| 章节 | 页码 | 内容 |
|---|---:|---|
| Chapter 17 - Data Transfer | 233–236 | 入口、流程、高级选项、Transfer Mode、Profile |
| Chapter 18 - Data Synchronization | 237–239 | 入口、比较选项、映射/条件、结果视图、部署选项、Oracle 限制 |
| Chapter 19 - Structure Synchronization | 240–242 | 入口、比较选项、结果树、DDL 对比、部署选项 |
| 附录（profile 扩展名） | 约 12762–12791 行 | `.ndtfmysql` / `.ndsymysql` / `.nssymysql` 等 |

> 手册是随安装包提供的 `navicat.pdf`（385 页）；行号来自本地 PDF 文本提取结果，
> 仅用于定位，不是官方页码。
