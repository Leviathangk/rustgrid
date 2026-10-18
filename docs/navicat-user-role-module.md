# Navicat 用户/角色模块解析（MySQL / SQL Server / Oracle / PostgreSQL）

本文是对 **Navicat Premium Lite 18** 账户（用户/角色）与权限管理模块的静态解析记录，
覆盖 **MySQL/MariaDB、SQL Server、Oracle、PostgreSQL** 四个引擎，用于给 RustGrid 的
用户/角色模块（见 `docs/user-management.md`）提供**行为对照依据**。

> 本文只记录**观察到的实现事实**与**可复现的定位方式**，不复制 Navicat 的代码；
> 目的是让 RustGrid 在与它同类的服务器上表现出兼容的行为。

## 1. 分析对象与证据来源

| 项目 | 值 |
|---|---|
| 主程序 | `D:\Program Files\Navicat\Navicat Premium Lite 18\navicat.exe` |
| 版本 | `18.0.3.0`（`ProductName` = `Navicat Premium Lite`） |
| 格式 | 原生 PE x86-64，99,011,144 字节，**非 .NET、非 Electron** |
| SHA-256 | `4de35ae7dfbbc6f570867a14702fa97248c7b39560a1df891d3acc725442182b` |
| 引擎库 | 同目录 `libcc.dll`，版本 `1.1.0`，134,723,656 字节 |
| 分析方式 | REA（MCP）+ Ghidra 12.1.4 headless：`navicat.exe` 得到 **159,001 个函数 / 75,409 个字符串**（image base `0x400000`）；`libcc.dll` 用字节级抽取 |

**证据分级**（引用本文数据时请保留这一区分）：

- **REA 证据**：`navicat.exe` 的界面文案、函数与字符串总量 —— 由 REA/Ghidra 会话产出。
- **字节抽取证据**：`libcc.dll` 的全部 SQL 文本 —— 由对该 DLL 的**原始字节扫描**
  （ASCII + UTF-16 两种编码）取得；文中的**文件偏移可直接复现**，但不是 REA 的 Evidence。
- **重构（标为「重构」的内容）**：Navicat 的 SQL 由多条片段拼接，本文按片段给出结构，
  标为重构的部分是把片段串起来的合理还原，**不是逐字节还原的最终语句**（见第 12 节）。

## 2. 分层结构：界面在主程序，SQL 在引擎库

```
navicat.exe           界面层：动作、对话框、文案（UTF-16 字面量，内联在 .text）
                      ✔ "New User" / "Privilege Manager" / "Server Privileges" / …
                      ✘ 没有任何引擎的用户/角色 SQL

libcc.dll             引擎层：四个引擎的账户目录查询与 DDL/DCL（ASCII，约 0x6260000–0x6550000）
                      ✔ mysql.user / sys.server_principals / DBA_USERS / pg_roles …
```

对 `navicat.exe` 全文件按 ASCII 与 UTF-16 两种编码检索 `mysql.user`、`CREATE USER`、
`GRANT ALL PRIVILEGES`、`IDENTIFIED BY`、`tables_priv`、`authentication_string`，
**命中数全为 0**；同样检索在 `libcc.dll` 命中 —— 这是上述分层结论的直接依据。

### 2.1 界面文案为何不在字符串清单里

`navicat.exe` 的界面文案是**内联在 `.text` 的 UTF-16 字面量**，由字符串注册函数逐条写入
全局槽位：

```c
void FUN_008ba440(void) {            // 0x8ba440 - 0x8bc964，9,509 字节的字符串注册表
  FUN_00411cc0(&PTR_DAT_02ff0c28, L"No Starred Items");
  FUN_00411cc0(&PTR_DAT_02ff0c38, L"Not Activated");
  ...
}
```

因此 Ghidra 默认的 ASCII 字符串分析**不会**收录它们（`search_strings` 查不到），
需要按 UTF-16 字节模式检索或直接读内存。

### 2.2 引擎判定

Navicat 支持同一界面下的多引擎分支，且对**同引擎的不同大版本**另做分支，例如：

- 正则权限列的新旧拼写并存：`Reference_priv`（旧）与 `References_priv`（新）；
- SQL Server 同时有 `sys.server_principals`/`sys.sql_logins` 与遗留的 `sys.syslogins`
  （`l.denylogin`、`l.createdate`、`l.dbname`、`l.language`）；
- Oracle 同时有 `DBA_USERS` 与 `DBA_USERS a INNER JOIN SYS.SYSUSERS b`（老字典）；
- PostgreSQL 还留着 `UPDATE pg_authid SET rolcatupdate=`（PG 9.x 之前改超级用户标记）；
- MariaDB 专有列 `Delete_history_priv`、`Create_tablespace_priv`、`mysql.roles_mapping`。

这意味着**版本适配是这个模块的基本设计**，不是补丁。

## 3. 统一界面结构

截图对应 Navicat 的「用户」主标签页；四个引擎共用同一套界面与动作（均为 REA 证据）。

| 分类 | 文案 |
|---|---|
| 工具栏 / 右键 | `New User`、`Edit User`、`Delete User`、`Delete User Mapping`、`Privilege Manager`、`Open Privilege Manager`、`Manage Users` |
| 用户编辑器 | 对话框类名 **`TUserDesignForm`**；动作 `acRevokeExecute`、`acRevokeAllExecute`、`acRevokeSelectedExecute` |
| 权限面板 | `Add Privilege`、`Add Privilege Group`、`Add Privilege Groups with Batch`、`Server Privileges`、`Show Granted Privileges Only` |
| 资源限制 | `Max Queries Per Hour`、`Max Updates Per Hour`、`Max Connections Per Hour`、`Max User Connections`、`Connection Limit` |
| 认证 / TLS | `Authentication Method`、`Authentication Type`、`Authentication Restrictions`、`Authentication Password`、`Expire Password`、`Expire Password Policy`、`X509`、`SSL Specified Cipher`、`Cipher`、`Issuer`、`Subject`、编辑框 `edX509SubjectName` |
| 角色 | `Default Role`、`Database Role(s)`、`Application Role(s)`、`Built-In Roles`、`Can create roles` |
| 其它 | `Confirm Password`、`User Name`；提示 `Confirm Password does not match Password` |

**关键点**：这些文案**没有引擎前缀**。同一个「服务器权限」网格、同一个「Add Privilege Group /
with Batch」交互，在四个引擎下都出现，差异全部由「该引擎返回的权限目录」与「该引擎的读写 SQL」
承担 —— 与 RustGrid 的 `PrivilegeCatalog` + `Connection` 抽象是同一个思路。

## 4. 编辑器工作流（引擎无关骨架）

由上述动作与各引擎 SQL 的组合推断出的统一流程（**重构**）：

1. **列表**：一条查询列出账户。MySQL 列 `mysql.user`；SQL Server 列
   `sys.server_principals`（+ `sys.sql_logins`）；Oracle 列 `DBA_USERS`；PostgreSQL 列 `pg_roles`。
2. **打开编辑器**：按主键/身份读该账户的**属性**（认证、密码策略、资源限制、TLS、引擎专有项）
   → 再读**三类权限**：服务器级、数据库/schema 级、对象（表/列/例程）级 → 最后读**角色边**。
3. **权限目录**：服务器权限清单不是硬编码的，而是按引擎能力取：
   - SQL Server 直接问服务器：`sys.fn_builtin_permissions`；
   - MySQL 用固定 28 项 + 版本裁剪的列；
   - Oracle 用系统权限表；
   - PostgreSQL 用**角色属性**（SUPERUSER/CREATEDB/…）而非权限名。
4. **编辑**：只在内存里改状态；`权限` 页提供四个动作
   （`acRevokeExecute` / `acRevokeAllExecute` / `acRevokeSelectedExecute` + 逐条勾选）。
5. **保存**：按**差集**生成脚本 —— 账户属性 → 权限 `GRANT`/`REVOKE` → 角色
   `GRANT`/`REVOKE` → 刷新。MySQL 结尾发 `FLUSH PRIVILEGES`。

## 5. MySQL / MariaDB

### 5.1 账户属性：**按版本拼接列清单**

观察到的两种基线形态：

```sql
-- 形式 A：账户编辑器（常规属性）
SELECT * FROM mysql.user u
 WHERE u.Host = '<host>' AND u.User = '<user>'

-- 形式 B：权限页（显式列清单，避开整行宽表；随后按版本追加）
SELECT u.Host, u.User, u.Select_priv, u.Insert_priv, u.Update_priv, u.Delete_priv,
       u.Create_priv, u.Drop_priv, u.Reload_priv, u.Shutdown_priv, u.Process_priv,
       u.File_priv, u.Grant_priv, u.References_priv, u.Index_priv, u.Alter_priv
       <按版本追加下列片段> FROM mysql.user u
```

版本相关片段（同一文件里**新旧拼写并存**，是版本适配的直接证据）：

| 片段 | 说明 |
|---|---|
| `, u.Show_db_priv, u.Super_priv, u.Create_tmp_table_priv, u.Lock_tables_priv, u.Execute_priv, u.Repl_slave_priv, u.Repl_client_priv` | 5.0+ 补齐的全局权限 |
| `, u.Create_view_priv, u.Show_view_priv, u.Create_routine_priv, u.Alter_routine_priv, u.Create_user_priv, u.Event_priv, u.Trigger_priv` | 5.1+（EVENT/TRIGGER/ROUTINE） |
| `, u.Create_tablespace_priv` | 5.7+ |
| `, u.Delete_history_priv` | **MariaDB 专有** |
| `, max_questions, max_updates, max_connections, super_priv` | 资源限制 + 超级用户 |
| `, max_user_connections` | 5.7+ 才有该列 |
| `, password_lifetime` | 5.7+ |
| `, plugin, password_expired` | 5.6+（`password_expired` 仅 MySQL） |
| `, ssl_type, ssl_cipher, x509_issuer, x509_subject` | TLS |

另有 `SELECT PRIVILEGE_TYPE FROM information_schema.USER_PRIVILEGES WHERE GRANTEE = ?` 与
MySQL 8 动态权限探测：

```sql
select * from information_schema.USER_PRIVILEGES where GRANTEE = '<user>@<host>'
select * from information_schema.USER_PRIVILEGES where PRIVILEGE_TYPE = 'SYSTEM_USER'
```

### 5.2 三类权限

```sql
-- 库级（注意新旧拼写二选一）
SELECT db.Host, db.Db, db.User, db.Select_priv, db.Insert_priv, db.Update_priv, db.Delete_priv,
       db.Create_priv, db.Drop_priv, db.Grant_priv
       [, db.Reference_priv | , db.References_priv]
       [, db.Index_priv, db.Alter_priv]
       [, db.Create_tmp_table_priv, db.Lock_tables_priv]
       [, db.Create_view_priv, db.Show_view_priv, db.Create_routine_priv, db.Alter_routine_priv, db.Execute_priv]
       [, db.Event_priv, db.Trigger_priv]
 FROM mysql.db db WHERE db.Host = '<host>' AND db.User = '<user>'

-- 表级（只取真正有权限的行）
SELECT tp.Host, tp.Db, tp.User, tp.Table_name, tp.Table_priv
  FROM mysql.tables_priv tp WHERE tp.Table_priv != ''
   AND tp.Host = '<host>' AND tp.User = '<user>' ORDER BY tp.User

-- 列级
SELECT cp.Host, cp.Db, cp.User, cp.Table_name, cp.Column_name, cp.Column_priv
  FROM mysql.columns_priv cp WHERE cp.Host = '<host>' AND cp.User = '<user>' ORDER BY cp.User

-- 例程级
SELECT pp.Host, pp.Db, pp.User, pp.Routine_name, pp.Routine_type, pp.Proc_priv
  FROM mysql.procs_priv pp WHERE pp.Host = '<host>' AND pp.User = '<user>' ORDER BY pp.User
```

表级另有走 `information_schema` 的形态：
`SELECT GRANTEE, TABLE_SCHEMA, TABLE_NAME, PRIVILEGE_TYPE, IS_GRANTABLE FROM information_schema.TABLE_PRIVILEGES WHERE GRANTEE = "…"`。
`IS_GRANTABLE` 是判断「带 `WITH GRANT OPTION`」的依据
（`GRANT USAGE ON db.* WITH GRANT OPTION` 这类只挂条目的库，MySQL 只报 `USAGE` + 可授权标志）。

### 5.3 角色与 `global_priv`

```sql
-- MySQL 8
SELECT FROM_HOST, FROM_USER, TO_HOST, TO_USER FROM mysql.role_edges
SELECT DEFAULT_ROLE_HOST, DEFAULT_ROLE_USER, HOST, USER FROM mysql.default_roles
SET DEFAULT ROLE <role> TO <account>   /   SET DEFAULT ROLE NONE TO <account>
GRANT ROLE <role> TO <member>          /   REVOKE ROLE <role> FROM <member>

-- MariaDB（以及旧版本）
SELECT Host, User, Role FROM mysql.roles_mapping
SELECT Role, User, Host FROM mysql.roles_mapping

-- MySQL 8：密码策略在 JSON 列里
Select User, Host, JSON_EXTRACT(Priv, '$.password_lifetime') as password_lifetime
  from mysql.global_priv;
```

### 5.4 写入语句

```sql
CREATE USER <account> …        ALTER USER <account> …        DROP USER <account>
CREATE USER IF NOT EXISTS <account> …
ALTER USER USER() IDENTIFIED BY '<pwd>'          -- 改当前用户密码
SET PASSWORD = PASSWORD('<pwd>')                 SET PASSWORD FOR <account> = PASSWORD('<pwd>')
SET PASSWORD FOR <account> = OLD_PASSWORD('<old>')   -- 改密（旧口令校验）

-- 认证 / 密码策略子句（注意：**没有** `IDENTIFIED WITH <plugin>`）
IDENTIFIED | IDENTIFIED BY '<pwd>' | IDENTIFIED BY NULL | NOT IDENTIFIED
PASSWORD EXPIRE DEFAULT | PASSWORD EXPIRE NEVER | PASSWORD EXPIRE INTERVAL <n> DAY

-- 资源限制 / TLS / 重命名（与 RustGrid 同形）
WITH MAX_QUERIES_PER_HOUR … MAX_UPDATES_PER_HOUR … MAX_CONNECTIONS_PER_HOUR … MAX_USER_CONNECTIONS …
REQUIRE NONE | REQUIRE SSL | REQUIRE X509 | REQUIRE CIPHER '<c>' AND ISSUER '<i>' AND SUBJECT '<s>'
RENAME USER <old> TO <new>

-- 角色
GRANT ROLE <role> TO <member>   /   REVOKE ROLE <role> FROM <member>
SET DEFAULT ROLE <role> TO <account>   /   SET DEFAULT ROLE NONE TO <account>

-- 权限子句
GRANT <privs> ON <scope> TO <account> [WITH GRANT OPTION]
REVOKE <privs> ON <scope> FROM <account>
REVOKE GRANT OPTION FOR <privs> ON <scope> FROM <account>
GRANT USAGE ON *.* TO <account>                  -- 「只建账号、不给权限」的占位授权
FLUSH PRIVILEGES
```

**MySQL 特有的一点**：写入不只走 DCL。片段池里同时存在对授权表的**直接 UPDATE**：

```
mysql.user / mysql.db / mysql.tables_priv / mysql.columns_priv
SET User='…', Host ='…'  WHERE User='…' AND Host ='…'
```

即某些属性（尤其是 `mysql.db` 这类按库的逐列权限位）是直接改表再 `FLUSH PRIVILEGES`，
而不是逐个 `GRANT`。RustGrid 只走 DCL，功能等价但语句形态不同。

### 5.5 Navicat 在 MySQL 上没有独立的「角色」对象（对照事实）

值得注意的是：**Navicat 只在 Oracle 与 SQL Server 上提供独立的角色对象列表/设计器**，
MySQL/MariaDB 没有。证据是 `navicat.exe` 里的设计器 form ID：

| form ID | 引擎 |
|---|---|
| `ROLEDESIGN_GENERAL_ORA_80…112`、`ROLEDESIGN_GRANT_ORA_*` | Oracle |
| `ROLEDESIGN_GENERAL_MSSQL_80…110`、`ROLEDESIGN_ROLE_MSSQL_*` | SQL Server（角色设计器） |
| `SVRROLEDESIGN_*` / `DBROLEDESIGN_*` / `APPROLEDESIGN_*` | SQL Server（服务器角色 / 数据库角色 / 应用程序角色） |
| （无 `ROLEDESIGN_*_MYSQL_*`、`..._MARIADB`、`..._PGSQL_*`） | — |

用户设计器的分区跨引擎只有 `GENERAL` / `GRANT` / `ROLE` 三种，而 **MySQL/MariaDB 只拥有
`USERDESIGN_GENERAL_MYSQL_*` 与 `USERDESIGN_GRANT_MYSQL_*`，没有 `USERDESIGN_ROLE_*`**
（SQL Server 才有 `USERDESIGN_ROLE_MSSQL_*`）。

两条原因：其一，**Navicat 的 MySQL 用户设计器版本键只到 `MYSQL_56`**（4.1/5.0/5.1/5.4/5.5/5.6），
是在 MySQL 5.6 时代定型的，而 5.6 根本没有角色（8.0 才引入）；其二，MySQL 8 的角色**就是
`mysql.user` 里的账号**，没有独立角色字典（不像 Oracle 的 `DBA_ROLES`），按"独立对象才给独立
列表"的模型自然就不单列。

因此 §5.3/§5.4 里那些 `mysql.role_edges` / `default_roles` / `SET DEFAULT ROLE` 的用途是
**"把角色授予某个账号"**，而不是角色对象管理 —— 在 Navicat 里给 MySQL 建角色得自己写
`CREATE ROLE`。RustGrid 选择把角色做成一等对象，是**比 Navicat 的 MySQL 分支更完整**的有意差异。

## 6. SQL Server

### 6.1 登录列表（重构）

由以下片段拼成（片段即文件中的原文）：

```
FROM sys.server_principals p
LEFT JOIN sys.sql_logins l ON l.principal_id = p.principal_id
LEFT JOIN sys.syslanguages lan ON lan.name = p.default_language_name
LEFT JOIN sys.credentials cre ON p.credential_id = cre.credential_id
p.name, p.principal_id, p.type, p.is_disabled,
p.create_date, p.modify_date, p.default_database_name, p.default_language_name,
l.is_policy_checked, l.is_expiration_checked,
l.is_policy_checked, l.is_expiration_checked, cert.name AS certificate, ak.name AS asymmkey
LOGINPROPERTY(p.name, 'IsExpired') AS is_expired,
LOGINPROPERTY(p.name, 'IsLocked') AS is_locked,
LOGINPROPERTY(p.name, 'IsMustChange') AS is_must_changed,
```

即：登录名 + 类型 + 是否禁用 + 创建/修改时间 + 默认库/默认语言 + **密码策略**
（`CHECK_POLICY`/`CHECK_EXPIRATION`）+ **证书/非对称密钥**（`FROM CERTIFICATE` / `FROM ASYMMETRIC KEY`
建立的登录）+ **凭据** + 由 `LOGINPROPERTY()` 现算的**已过期/已锁定/须改密**三态。

**旧版本分支**（SQL Server 2000/2005，`sys.syslogins`）：
`l.name, l.denylogin AS is_disabled, l.createdate AS create_date, l.updatedate AS modify_date`、
`l.dbname AS default_database_name, l.language AS default_language_name`。

辅助查询：`SELECT sid, name FROM sys.sql_logins`（SID 对齐用）、
`SELECT name FROM sys.credentials ORDER BY name`、
`SELECT p.name, cre.name AS credential FROM sys.server_principals p INNER JOIN sys.credentials cre ON p.credential_id = cre.credential_id`。

### 6.2 服务器权限

```sql
-- 服务器级权限目录（由服务器给出，不是硬编码）
SELECT permission_name FROM sys.fn_builtin_permissions(DEFAULT)
 WHERE CLASS_DESC = 'Server' ORDER BY PERMISSION_NAME

-- 某主体已持有的服务器权限（state 区分 G/W/D）
SELECT prin.name, perm.class, endpts.name endpoint_name, login.name login_name,
       perm.type, perm.permission_name, perm.state
  FROM sys.server_permissions perm
 INNER JOIN sys.server_principals prin ON perm.grantee_principal_id = prin.principal_id
 LEFT OUTER JOIN sys.server_principals login ON perm.major_id = login.principal_id
```

`perm.state` 就是 RustGrid 所说的 `G`=授予 / `W`=授予且可转授 / `D`=拒绝 三态。
`class` 决定安全对象类别（100 = 服务器，101 = 登录，105 = 终端节点），
`endpoint_name`/`login_name` 列因此按 class 取值 —— 对应界面里的「终端节点权限 / 登录权限」分区。

### 6.3 数据库用户与对象权限

```sql
-- 数据库级权限目录
SELECT permission_name FROM sys.fn_builtin_permissions('DATABASE') ORDER BY permission_name

-- 库内主体（用户/角色）与 登录 ↔ 用户映射
FROM sys.database_principals dp
LEFT JOIN sys.server_principals log ON dp.sid = log.sid
LEFT JOIN sys.database_principals owner ON dp.owning_principal_id = owner.principal_id

-- 数据库角色成员
FROM sys.database_role_members m
LEFT JOIN sys.database_principals r ON m.role_principal_id = r.principal_id
LEFT JOIN sys.database_principals u ON m.member_principal_id = u.principal_id

-- 服务器角色成员
FROM sys.server_role_members m
LEFT JOIN sys.server_principals r ON m.role_principal_id = r.principal_id
LEFT JOIN sys.server_principals u ON m.member_principal_id = u.principal_id

-- 库内权限（class='3' 表示列级）
SELECT u.name, p.class, p.permission_name, p.state, o.type,
       CASE WHEN class='3' THEN '' ELSE o.name END AS ObjectName, c.name AS ColumnName
  FROM sys.database_permissions p
```

`LEFT JOIN sys.server_principals log ON dp.sid = log.sid` 正是界面里**「用户映射」**页的来源
（库用户 ↔ 服务器登录按 SID 配对）；这也是为什么它同时需要
`SELECT sid, name FROM sys.sql_logins` 作为对照表。

### 6.4 链接服务器登录映射与凭据

```sql
CAST((CASE WHEN l.local_principal_id = 0 THEN 1 ELSE 0 END) AS bit) AS is_public,
p.name AS local_user, l.uses_self_credential, l.remote_name AS remote_user
  FROM sys.linked_logins l
  LEFT JOIN sys.sql_logins p ON p.principal_id = l.local_principal_id
 WHERE l.server_id = (SELECT s.server_id FROM sys.servers s WHERE s.name = N'…')

INNER JOIN sys.credentials cre ON pc.credential_id = cre.credential_id
INNER JOIN sys.server_principals p ON pc.principal_id = p.principal_id
```

即「链接服务器登录映射」页（`local_principal_id = 0` 表示 public 映射）与
「凭据 → 主体」的反查。

### 6.5 写入语句

```
CREATE LOGIN <name> …                       DROP LOGIN <name>
ALTER LOGIN <name> …                        -- NAME= / DEFAULT_DATABASE= / DEFAULT_LANGUAGE=
                                            -- CHECK_POLICY= / CHECK_EXPIRATION= / MUST_CHANGE
                                            -- OLD PASSWORD= / ADD CREDENTIAL / DROP CREDENTIAL

-- 登录来源（与密码或证书/密钥绑定）
CREATE LOGIN …  FROM WINDOWS
CREATE LOGIN …  FROM CERTIFICATE <cert>
CREATE LOGIN …  FROM ASYMMETRIC KEY <key>

-- 角色：新语法与遗留存储过程并存
CREATE / ALTER / DROP SERVER ROLE …        （ALTER SERVER ROLE … ADD/DROP MEMBER）
CREATE / ALTER / DROP APPLICATION ROLE …
EXEC sp_addrolemember   N'<role>', N'<member>'   /  EXEC sp_droprolemember  …
EXEC sp_addsrvrolemember @loginame='…', @rolename = N'…'   -- 服务器角色（遗留）
EXEC sp_dropsrvrolemember @loginame='…'

-- SQL Server 2000 遗留路径（同一模块内并存）
EXEC sp_addlogin @loginame = …, @passwd=…, @defdb = …, @deflanguage = …
EXEC sp_revokelogin @loginame = …
EXEC dbo.sp_defaultlanguage @loginame = …, @language = …
EXEC dbp.sp_defaultdb @loginame = …, @defdb =
```

**权限语句的安全对象前缀**（与界面分区一一对应）：

```
 ON LOGIN::<name>        ON ENDPOINT::<name>        ON SCHEMA::<name>        ON OBJECT::<name>
DENY <perm> …            REVOKE GRANT OPTION FOR …            … WITH GRANT OPTION
```

`GRANT` / `DENY` / `REVOKE GRANT OPTION FOR` + `WITH GRANT OPTION` 的四件套解释了
界面为什么需要三态（授予 / 授予且可转授 / 拒绝），以及为什么 `DENY` 只在这一引擎出现。

> 注意：`MUST_CHANGE_PASSWORD = …`（带 `_PASSWORD`）**属于 Snowflake 分支**，不要与这里的
> SQL Server `MUST_CHANGE` 混淆。

权限名（来自 `fn_builtin_permissions`）里可见 `ALTER ANY SERVER ROLE`、`ALTER ANY APPLICATION ROLE`
等 —— 服务器角色的管理权限也在同一套目录里，所以界面不需要为角色单列一套权限名。

Azure/PDW 变体（`CREATE LOGIN AZURE SQL`、`ALTER LOGIN SQL SERVER CONTEXT`、
`CREATE USER AZURE SQL DW` 等）是同一分支的语法差异，界面不变。

## 7. Oracle

### 7.1 用户列表（重构）

```
FROM SYS.DBA_USERS a
USERNAME, USER_ID, CREATED, ACCOUNT_STATUS, LOCK_DATE, EXPIRY_DATE,
DEFAULT_TABLESPACE, TEMPORARY_TABLESPACE, PROFILE, INITIAL_RSRC_CONSUMER_GROUP,
EXTERNAL_NAME, PASSWORD_VERSIONS, EDITIONS_ENABLED, AUTHENTICATION_TYPE, ORACLE_MAINTAINED
FROM SYS.ALL_USERS
SELECT USERNAME, CASE WHEN (USERNAME = USER) THEN 1 ELSE 0 END ISCURRENTUSER FROM SYS.ALL_USERS
-- 老字典
FROM SYS.DBA_USERS a INNER JOIN SYS.SYSUSERS b ON a.USER_ID = b.ID
USERNAME, USERID, CREATED, PASSWORD, ACCOUNT_STATUS, LOCK_DATE, EXPIRY_DATE,
DEFAULT_TABLESPACE, TEMPORARY_TABLESPACE, INITIAL_RSRC_CONSUMER_GROUP, EXTERNAL_NAME, IS_LOCKED
```

即：身份 + **账户状态/锁定/过期**（`ACCOUNT_STATUS`、`LOCK_DATE`、`EXPIRY_DATE`）+
**默认/临时表空间** + **PROFILE** + 资源消费组 + 外部名 + 密码版本 +
认证类型（`PASSWORD` / `EXTERNAL` / `GLOBAL`）+ `ORACLE_MAINTAINED`（是否系统自带）。
`ALL_USERS` 变体用 `USERNAME = USER` 标出"当前用户"，暴露它是**可见性受限**的降级路径。

### 7.2 角色与权限

```sql
-- 角色名册与角色授予（DEFAULT_ROLE 即界面「Default Role」）
FROM SYS.DBA_ROLES
SELECT USERNAME, GRANTED_ROLE, ADMIN_OPTION, DEFAULT_ROLE FROM SYS.USER_ROLE_PRIVS
SELECT GRANTEE,  GRANTED_ROLE, ADMIN_OPTION, DEFAULT_ROLE FROM SYS.DBA_ROLE_PRIVS

-- 系统权限（含 SYSDBA/SYSOPER 的特殊合并）
SELECT PRIVILEGE, ADMIN_OPTION FROM "SYS"."USER_SYS_PRIVS"
SELECT PRIVILEGE, ADMIN_OPTION FROM "SYS"."DBA_SYS_PRIVS" WHERE GRANTEE = '…'
SELECT USERNAME GRANTEE, PRIVILEGE, ADMIN_OPTION FROM "SYS"."USER_SYS_PRIVS"
 UNION SELECT USERNAME, 'SYSDBA', 'NO' FROM v$pwfile_users WHERE SYSDBA = 'TRUE'
 UNION SELECT USERNAME, 'SYSOPER', 'NO' FROM v$pwfile_users WHERE SYSOPER = 'TRUE'
 ORDER BY GRANTEE
SELECT PRIVILEGE FROM SYS.SESSION_PRIVS      -- 当前会话权限（用于「我能不能改」）

-- 对象 / 列权限
SELECT PRIV.GRANTOR, PRIV.GRANTEE, PRIV.TABLE_NAME, OBJ.OBJECT_TYPE, PRIV.PRIVILEGE, PRIV.GRANTABLE
  FROM SYS.DBA_TAB_PRIVS PRIV INNER JOIN SYS.DBA_OBJECTS OBJ
    ON PRIV.TABLE_NAME = OBJ.OBJECT_NAME AND PRIV.OWNER = OBJ.OWNER
SELECT GRANTEE, OWNER, TABLE_NAME, COLUMN_NAME, PRIVILEGE, GRANTABLE FROM SYS.DBA_COL_PRIVS
-- ALL_* 变体（当前用户可见范围）+ 物化视图变体（LEFT JOIN SYS.USER_MVIEWS）

-- 表空间配额与 PROFILE 限制
SELECT TABLESPACE_NAME, MAX_BYTES FROM SYS.DBA_TS_QUOTAS WHERE USERNAME = '…'
SELECT TABLESPACE_NAME, MAX_BYTES FROM SYS.USER_TS_QUOTAS
-- DBA_PROFILES：FAILED_LOGIN_ATTEMPTS / PASSWORD_LIFE_TIME 等

-- 原始 DDL（编辑器「定义」页）
SELECT DBMS_METADATA.GET_DDL('USER', …)   /   ('ROLE', …)
```

**SYSDBA/SYSOPER 不在 `DBA_SYS_PRIVS` 里** —— 它们由口令文件
（`v$pwfile_users`）决定，所以 Navicat 必须把两个来源 `UNION` 起来才能显示"完整"的服务器权限。
这是 Oracle 分支最容易漏掉的一点。

### 7.3 写入语句

```
CREATE USER …   ALTER USER …   DROP USER …
CREATE ROLE …   ALTER ROLE …   DROP ROLE …

IDENTIFIED BY <pwd> | IDENTIFIED EXTERNALLY | IDENTIFIED GLOBALLY
DEFAULT TABLESPACE <ts>      QUOTA <n> ON <ts>      PROFILE <profile>
ACCOUNT LOCK / ACCOUNT UNLOCK        ALTER PROFILE …
GRANT <priv> TO <user> [WITH ADMIN OPTION]   /   GRANT <role> TO <user>
```

系统权限名可见 `GRANT ANY PRIVILEGE`、`GRANT ANY ROLE`、`GRANT ANY TABLE`、`GRANT ANY VIEW`、
`GRANT ANY PROCEDURE`、`GRANT ANY SEQUENCE`、`GRANT ANY PACKAGE`、`GRANT ANY DOMAIN`、
`GRANT ANY CONTEXT`、`GRANT ANY OBJECT PRIVILEGE` —— 即 Oracle 的「ANY 系列」系统权限。

## 8. PostgreSQL

### 8.1 角色列表（PG 没有"用户"与"角色"之分，统一是 ROLE）

```
FROM pg_roles
rolname AS name, oid AS oid, rolsuper AS is_superuser, rolinherit AS inherit_privileges,
rolcreaterole AS can_create_role, rolcreatedb AS can_create_database,
rolcanlogin AS can_login, rolconnlimit AS connection_limit, rolvaliduntil AS expiry_date,
rolconfig, rolreplication AS can_replicate, rolbypassrls AS can_bypass_rls,
pg_catalog.shobj_description(oid, 'pg_authid') AS comment
```

即界面上的「角色属性」勾选项一一对应：
**超级用户 / 继承权限 / 创建角色 / 创建数据库 / 可登录 / 复制 / 绕过行级安全 / 连接限制 / 到期时间**，
外加 `rolconfig`（角色级 GUC 参数，界面里就是 `ALTER ROLE … SET …` 那部分）。
注释走 `pg_authid` 的共享描述，不是 `pg_roles`。

### 8.2 角色成员关系

```sql
SELECT r1.rolname AS role, r2.rolname AS member_name, m.admin_option AS admin_option
  FROM pg_roles r1, pg_auth_members m, pg_roles r2
 WHERE r1.oid = m.roleid AND m.member = r2.oid
```

方向：`r1` = 被授予的角色，`r2` = 成员，`admin_option` 即 `WITH ADMIN OPTION`。
一次性读出整张图，再在内存里按角色过滤成「成员属于 / 成员」两侧 —— 与 MySQL 的
`role_edges` 处理方式一致。

### 8.3 权限：直接扫 ACL 数组，而**不用** `has_*_privilege()`

```sql
-- 表/视图/序列/物化视图/外部表的 ACL
SELECT rel.relname, nsp.nspname, rel.relkind, rel.relacl
  FROM pg_class rel INNER JOIN pg_namespace nsp ON rel.relnamespace = nsp.oid
 WHERE rel.relkind IN ('r','v','S','m','f')
   AND rel.relacl IS NOT NULL
   AND rel.relacl::varchar LIKE E'%…'          -- 用角色名/oid 在 acl 文本里匹配

SELECT c.oid, n.nspname AS schemaname, c.relname AS tablename, c.relacl,
       pg_get_userbyid(c.relowner) AS tableowner, obj_description(c.oid) AS description,
       c.relkind, ci.relname As cluster
  ...（视图变体用 pg_get_viewdef(c.oid, true) AS definition）

-- schema 的 ACL
SELECT oid, nspname AS schemaname, nspacl, pg_get_userbyid(nspowner) AS schemaowner
  FROM pg_namespace

-- 其它安全对象的 ACL
fs.srvacl AS acl      -- pg_foreign_server
w.fdwacl              -- pg_foreign_data_wrapper
cl.relacl AS acl      -- 序列
```

**关键设计**：PostgreSQL 的授权是「ACL 数组」而不是"权限行"，Navicat 的办法是
**读出 `relacl`/`nspacl` 字段后用 `LIKE` 在 acl 文本里匹配角色**，而不是对每个对象调
`has_table_privilege()`。这解释了为什么界面能一次性列出"这个角色在哪些对象上有权限"。

### 8.4 写入与默认权限

```
CREATE ROLE …    ALTER ROLE …    DROP ROLE …    ALTER ROLE SET …
WITH PASSWORD = '…'      PASSWORD = '…'      VALID UNTIL '…'      CONNECTION LIMIT = …
NOLOGIN / LOGIN          ALTER DEFAULT PRIVILEGES …
```

`rolvaliduntil` ↔ `VALID UNTIL`、`rolconnlimit` ↔ `CONNECTION LIMIT`、`rolconfig` ↔ `ALTER ROLE SET`
都是一一对应的读写对。`ALTER DEFAULT PRIVILEGES` 是「默认权限」页的落点。
遗留：`UPDATE pg_authid SET rolcatupdate=`（PG 9.x 之前的超级用户标记）。

## 9. 四引擎对照

| | MySQL / MariaDB | SQL Server | Oracle | PostgreSQL |
|---|---|---|---|---|
| 身份 | `user@host` | 登录名 | 用户名 | 角色名（无 user/role 之分） |
| 账户目录 | `mysql.user` | `sys.server_principals` + `sys.sql_logins` | `DBA_USERS`（降级 `ALL_USERS`） | `pg_roles` |
| 服务器权限 | `mysql.user` 的 `*_priv` 列 + `information_schema.USER_PRIVILEGES` | `sys.server_permissions`（`state` = G/W/D） | `DBA_SYS_PRIVS` + `v$pwfile_users`（SYSDBA/SYSOPER） | 角色属性（`rolsuper` 等） |
| 权限目录来源 | 硬编码 28 项 + 版本裁剪 | **问服务器** `sys.fn_builtin_permissions` | 系统权限表 | 角色属性布尔位 |
| 库级 | `mysql.db`（`db.*`） | `ON DATABASE::`（class 0） | —（Oracle 无库级，直接 schema=用户） | `ON DATABASE`（CONNECT/CREATE/TEMPORARY） |
| schema 级 | — | — | 对象属主即 schema | `ON SCHEMA`（`nspacl`） |
| 对象级 | `tables_priv`/`columns_priv`/`procs_priv` | `sys.database_permissions`（class 3 = 列） | `DBA_TAB_PRIVS`/`DBA_COL_PRIVS` | `pg_class.relacl`（LIKE 扫描） |
| 角色 | `role_edges`（MySQL 8）/ `roles_mapping`（MariaDB） | 服务器角色 + 数据库角色 + 应用程序角色 | `DBA_ROLES` + `DBA_ROLE_PRIVS` | `pg_auth_members` |
| 默认角色 | ✅ `mysql.default_roles` / `SET DEFAULT ROLE` | — | ✅ `DEFAULT_ROLE` 列 | — |
| DENY | — | ✅ | — | — |
| 用户映射 | — | ✅ 库用户 ↔ 登录（按 SID） | —（有 `USER MAPPING`，用于外部服务器） | — |
| 配额/资源限制 | `max_*` 列 | — | `DBA_TS_QUOTAS` + `PROFILE` | `rolconnlimit` |
| 密码策略 | `password_lifetime`/`password_expired`（MySQL 8 另查 `global_priv`） | `CHECK_POLICY`/`CHECK_EXPIRATION` + `LOGINPROPERTY()` | `PROFILE` 的 `PASSWORD_LIFE_TIME` | `VALID UNTIL` |
| 原始 DDL | `SHOW CREATE USER` | — | `DBMS_METADATA.GET_DDL` | `pg_get_viewdef`/`pg_get_userbyid` |

## 10. 与 RustGrid 的对应

### 10.1 已经一致

| 方面 | 说明 |
|---|---|
| 权限来源 | 引擎各取自己的目录视图（`information_schema` / `sys.*` / `DBA_*` / `pg_*`），UI 只在目录上渲染 |
| 差集写入 | 都是「读原状态 → 编辑 → 按差集发 GRANT/REVOKE/ALTER」，不是全量重建 |
| 角色边 | 都是整图读出后在内存里分「成员属于 / 成员」两侧 |
| 引擎差异位置 | 都在驱动内，UI 无引擎判断（RustGrid 通过 `UserEditorSpec` + `PrivilegeCatalog` 下发） |

### 10.2 已按本文采纳

1. **MySQL 账户查询按目录自适应**（对应 5.1，`crates/rustgrid-mysql/src/user.rs`）：新增
   `CatalogShape`，用一次 `information_schema.COLUMNS` 探测 `mysql.user` 实际有哪些列，缺列
   投影为 `NULL AS <列名>`，使 MySQL 5.6 / 各版本 MariaDB 不再导致整个「用户」标签页失败；
   探测失败时回退到"假定列齐全"。
2. **MariaDB 角色边**（对应 5.3）：`mysql.role_edges` 缺失时改读 `mysql.roles_mapping`，
   把 `Member@Host` ↔ `Role` 映射为同一条角色边（MariaDB 无角色主机与 admin option，取
   `@'%'` 与 `false`）。
3. **账户类型 `AccountKind`**（对应 6/7/8 各引擎的"主体是什么"）：新增
   `UserAccount.kind` + `UserEditorSpec.kinds`，四引擎各自如实上报 —— MariaDB 读
   `mysql.user.is_role`、MySQL 8 由 `mysql.role_edges` 推断、SQL Server 用
   `sys.server_principals.type_desc`（登录名 / Windows 组 / 服务器角色）、Oracle 由
   `DBA_ROLES` 判定、PostgreSQL 用 `rolcanlogin`。
4. **用户 / 角色 作用域开关**（对应本文截图里 Navicat 的「用户」下拉）：Users 标签页工具栏
   最左边多一个 用户 / 角色 两段开关（`UserScope`），切换即换整个列表、工具栏文案
   （新建角色 / 编辑角色 / 删除角色）、新建窗口的标题与预选类型、以及底部计数；两种布局都用
   图标区分类型（单人 / 双人 / 盾牌），**平铺网格（默认布局）下也能分辨**。
   开关是否出现由 `Connection::roles_supported()` 决定（MySQL 8 / MariaDB 为真，MySQL ≤ 5.7 为假），
   所以没有角色机制的服务端不会出现死开关。
5. **角色的创建 / 编辑 / 删除**（对应 5.4、6.5、7.3）：`CREATE ROLE`、
   `ALTER ROLE … WITH NAME`（SQL Server）/ `ALTER ROLE … RENAME TO`（Oracle）、`DROP ROLE`
   按类型分派；角色**不需要**新的权限页面 —— 服务器权限 / 权限 / 角色三页原样复用于角色。
6. **Oracle 的 SYSDBA / SYSOPER**（对应 7.2）：从 `v$pwfile_users` 读入并并入服务器权限，
   作为「特权管理」分组里的两个可勾选项；**不放进"全部系统权限"预设**，避免一键授予 DBA。

### 10.3 与 Navicat 的有意差异

| 差异 | 理由 |
|---|---|
| MySQL 权限走 `information_schema` 而非 `mysql.db`/`*_priv` 表 | 更干净，且天然处理 `IS_GRANTABLE` 的边角情况 |
| 不直接 UPDATE `mysql.*` 授权表 | Navicat 部分路径直接改表再 `FLUSH PRIVILEGES`；RustGrid 只走 DCL，功能等价且可预览 |
| 发 `GRANT USAGE ON *.* TO` 作为「无权限」占位 | MySQL 的 `CREATE USER` 已隐含该权限，多一条只让 SQL 预览变噪 |
| 单独查 `mysql.global_priv` | MySQL 8 的 `mysql.user` 视图已暴露 `password_lifetime` |
| MySQL 默认角色（`SET DEFAULT ROLE`）/ Oracle `DEFAULT_ROLE` | 需要在「角色」页加一列状态与写入语句，尚未实现 |
| MySQL 专有权限列（`Delete_history_priv` 等） | RustGrid 只列各版本通用权限，避免"点了会失败"的死开关 |
| SQL Server 权限目录用固定 13 项而非查 `fn_builtin_permissions` | `Connection::privilege_catalog` 目前是**同步**方法、在渲染期调用；改成问服务器需要先把它异步化，属于跨全部驱动的改造，留作后续 |
| MySQL 8 角色识别是**推断** | MySQL 8 的 `mysql.user` 没有 `is_role` 列；只能认"被授予给别人"的账号。没有任何被授予者的角色会被当成用户（MariaDB 用 `is_role`，无此问题） |

> 四个驱动在 Navicat 的对应查询上已基本覆盖（见 `docs/user-management.md` 的「各引擎映射」
> 与「驱动实现要点」）；本轮补齐的是**账户类型**这一跨引擎维度，以及 Oracle 的口令文件权限。

## 11. 复现方法

`libcc.dll` 中的 SQL 是 ASCII，**表中偏移即该段文字在文件里的起始位置**：

```powershell
$bytes = [IO.File]::ReadAllBytes("D:\Program Files\Navicat\Navicat Premium Lite 18\libcc.dll")
$ascii = [Text.Encoding]::ASCII.GetString($bytes)
$ascii.IndexOf("FROM sys.server_principals p ")   # -> 104991616 (0x6420B80)
```

| 引擎 | 字符串 | 文件偏移 |
|---|---|---|
| MySQL | `SELECT * FROM mysql.user u` | `0x63F25F0` |
| MySQL | `SELECT u.Host, u.User, u.Select_priv, …` | `0x642B490` |
| MySQL | `, db.Reference_priv`（旧拼写） | `0x63F2170` |
| MySQL | `, db.References_priv`（新拼写） | `0x63F2188` |
| MySQL | ` FROM mysql.db db` | `0x63F2288` |
| MySQL | `SELECT tp.Host, … FROM mysql.tables_priv tp WHERE tp.Table_priv!= ''` | `0x63F28E0` |
| MySQL | `SELECT GRANTEE, TABLE_SCHEMA, … FROM information_schema.TABLE_PRIVILEGES` | `0x63F29D0` |
| MySQL | `SELECT cp.Host, … FROM mysql.columns_priv cp` | `0x63F2AA0` |
| MySQL | `SELECT pp.Host, … FROM mysql.procs_priv pp` | `0x63F2430` |
| MySQL | `SELECT FROM_HOST, FROM_USER, TO_HOST, TO_USER FROM mysql.role_edges` | `0x63F53A0` |
| MySQL | `SELECT DEFAULT_ROLE_HOST, … FROM mysql.default_roles` | `0x63F5450` |
| MySQL | `SELECT Host, User, Role FROM mysql.roles_mapping` | `0x642BAC8` |
| MySQL | `Select User, Host, JSON_EXTRACT(Priv, '$.password_lifetime') … from mysql.global_priv;` | `0x642CE70` |
| MySQL | `ALTER USER USER() IDENTIFIED BY '` | `0x633D410` |
| MySQL | `SET PASSWORD = PASSWORD('` | `0x633D5A0` |
| MySQL | `GRANT USAGE ON *.* TO` | `0x6541488` |
| MySQL | `CREATE USER IF NOT EXISTS ` | `0x6548D88` |
| SQL Server | `FROM sys.server_principals p ` | `0x6420B80` |
| SQL Server | `FROM sys.sql_logins l ` | `0x6420AE0` |
| SQL Server | `FROM sys.server_role_members m ` | `0x6421240` |
| SQL Server | `FROM sys.database_role_members m ` | `0x641B018` |
| SQL Server | `FROM sys.database_principals dp ` | `0x6418760` |
| SQL Server | `SELECT prin.name, perm.class, endpts.name endpoint_name, …` | `0x6422950` |
| SQL Server | `SELECT u.name, p.class, p.permission_name, p.state, …` | `0x6422B50` |
| SQL Server | `SELECT permission_name FROM sys.fn_builtin_permissions('DATABASE')` | `0x63CE770` |
| SQL Server | `CREATE LOGIN ` / ` FROM WINDOWS ` / ` FROM CERTIFICATE ` / ` FROM ASYMMETRIC KEY ` | `0x6543A20` / `0x6543A30` / `0x6543A40` / `0x6543A58` |
| SQL Server | ` ON LOGIN::` / ` ON ENDPOINT::` / ` ON SCHEMA::` / ` ON OBJECT::` | `0x6543B48` |
| SQL Server | `DENY ` / ` WITH GRANT OPTION` / `REVOKE GRANT OPTION FOR ` | `0x6543BA4` / `0x6543BB0` / `0x6543B88` |
| SQL Server | `EXEC sp_addsrvrolemember @loginame='` / `EXEC sp_dropsrvrolemember @loginame='` | `0x6543968` / `0x65439A8` |
| SQL Server | `EXEC sp_addlogin @loginame = `（2000 遗留）/ `EXEC sp_revokelogin @loginame = ` | `0x65439D0` / `0x6543A70` |
| MySQL | `SET DEFAULT ROLE` / `RENAME USER ` / `FLUSH PRIVILEGES` | `0x6541240` / `0x6541340` / `0x65413D8` |
| MySQL | `REQUIRE SSL` / `REQUIRE X509` / `REQUIRE NONE` / `CIPHER '` / `ISSUER '` / `SUBJECT '` | `0x6541260`–`0x65412C8` |
| MySQL | `MAX_QUERIES_PER_HOUR ` / `MAX_USER_CONNECTIONS ` | `0x65412D8` / `0x6541328` |
| MySQL | `mysql.user ` / `mysql.db ` / `mysql.tables_priv ` / `mysql.columns_priv `（直接 UPDATE） | `0x6541350`–`0x6541388` |
| MySQL | `SET User='` / `WHERE User='` / ` AND Host ='` | `0x65413A0` / `0x65413B8` / `0x65413C8` |
| Oracle | `FROM SYS.DBA_USERS ` | `0x64162E0` |
| Oracle | `FROM SYS.DBA_ROLES ` | `0x6410EB8` |
| Oracle | `SELECT GRANTEE, PRIVILEGE, ADMIN_OPTION FROM "SYS"."DBA_SYS_PRIVS"` | `0x640F690` |
| Oracle | `SELECT USERNAME, GRANTED_ROLE, ADMIN_OPTION, DEFAULT_ROLE FROM SYS.USER_ROLE_PRIVS` | `0x6410F60` |
| Oracle | `SELECT GRANTEE, OWNER, … FROM SYS.DBA_COL_PRIVS` | `0x640FFB0` |
| Oracle | `SELECT TABLESPACE_NAME, MAX_BYTES FROM SYS.DBA_TS_QUOTAS` | `0x6416400` |
| PostgreSQL | `FROM pg_roles ` | `0x6401775` |
| PostgreSQL | `SELECT r1.rolname AS role, r2.rolname AS member_name, …` | `0x6401720` |
| PostgreSQL | `rolname AS name, oid AS oid, rolsuper AS is_superuser, …` | `0x64018D0` |
| PostgreSQL | `SELECT c.oid, n.nspname AS schemaname, c.relname AS tablename, c.relacl` | `0x64044D0` |
| PostgreSQL | `ALTER DEFAULT PRIVILEGES` | `0x62609B8` |

`navicat.exe` 侧的界面文案是 UTF-16：

```powershell
$bytes = [IO.File]::ReadAllBytes("D:\Program Files\Navicat\Navicat Premium Lite 18\navicat.exe")
$wide  = [Text.Encoding]::Unicode.GetString($bytes)
$wide.IndexOf("Privilege Manager")
```

引擎层的源文件名（编译期路径泄漏，佐证模块划分）：

```
D:\dev\navicatlibs\libcc\src\cs_dataset_sql_provider_mysql.cpp
                             cs_preset_dataset_sql_provider_mysql.cpp
                             cs_server_json_forward_compatible_save_load_handler_mysql.cpp
                             cs_table_action_mysql.cpp
                             cm_server_settings_mysql.cpp
```

## 12. 未证实与边界

- **SQL 的确切拼接顺序与分支条件未反编译**：第 5–8 节列出的是**池中存在的片段**与可确定的
  前后缀；标「重构」的语句是按片段还原的合理结构，不是逐字节还原。要完全确定需反编译
  `libcc.dll` 的 SQL provider（约 1–2 小时 Ghidra 分析）。
- **未做运行时验证**：本文没有实测 Navicat 发出的报文。如需，可挂钩
  `libmariadb.dll` 的 `mysql_real_query`/`mysql_stmt_prepare` 或直接挂钩 `libcc.dll`。
- **"版本 vs 目录探测"未区分**：Navicat 看起来按版本拼接（新旧拼写并存），但究竟按版本号
  还是按目录探测决定，本文未能区分；RustGrid 选择目录探测，两者结果等价。
- **内部动作名不是 SQL**：形如 `GRANT STATEMENT`、`CREATE USER STATEMENT`、
  `ALTER USER CONTEXT` 的条目是界面动作/语法分组标识，不要当作可执行语句。
- **共享片段不属单一引擎**：`CREATE USER `、`ALTER USER `、`DROP USER `、`IDENTIFIED BY`
  等是四引擎共用的模板前缀，第 5–8 节只在明确带引擎专有子句时才归到该引擎下。
- **同一片段池里还有本文未覆盖的引擎**：例如 Snowflake 的 `MUST_CHANGE_PASSWORD = `、
  `DEFAULT_ROLE = `、`DEFAULT_WAREHOUSE = '`、`DAYS_TO_EXPIRY = `、`DEFAULT_SECONDARY_ROLES = ('`
  与 `svv_*` 视图，达梦的 `"SYS"."DBA_SYS_PRIVS"` 等。按关键字归类时先确认它属于哪个引擎簇
  （同一引擎的片段在文件里通常**连续成簇**，见第 11 节的偏移），否则容易张冠李戴。
- **`IDENTIFIED WITH <plugin>` 是一处填空**：Navicat 的 MySQL 写入路径只有
  `IDENTIFIED` / `IDENTIFIED BY …` / `NOT IDENTIFIED`，**没有**插件子句；插件在读取侧
  （`, plugin, password_expired`）出现，写入侧未见对应语句。RustGrid 用
  `IDENTIFIED WITH <plugin> BY …` 更完整，这是有意的差异，不是缺陷。

## 13. 相关文档

- `docs/user-management.md` —— RustGrid 账户与权限管理的实现（模型、驱动契约、UI、各引擎映射）。
- `AGENTS.md` —— 扩展性要求与 UI 约定（引擎差异不得渗入 UI 层）。
