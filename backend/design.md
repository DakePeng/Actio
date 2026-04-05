# ASR + 声纹识别系统功能与架构设计文档

## 1. 文档目的

本文档用于定义一套支持 **ASR（语音识别）+ 声纹识别/匹配/存储** 的系统方案，重点关注：

* 功能边界
* 系统分层
* 模块职责
* 数据流设计
* 本地 / 云端混合策略
* 核心数据模型
* 演进路线
* 错误与异常处理策略
* 可观测性设计

本文档不涉及具体代码实现细节、性能调优参数、部署命令与模型微调细节。

---

## 2. 设计目标

### 2.1 核心目标

构建一套可扩展的语音智能系统，具备以下能力：

1. **语音转写（ASR）**

   * 支持实时流式转写
   * 支持离线文件转写
   * 支持短语音与长语音两类场景

2. **声纹能力**

   * 支持说话人注册
   * 支持 1:1 说话人验证
   * 支持 1:N 说话人识别
   * 支持声纹向量存储与检索
   * 支持多人场景下的说话人映射

3. **混合推理能力**

   * 支持本地优先推理
   * 支持云端回退或增强
   * 支持根据设备能力、网络状态、隐私策略做动态路由

4. **工程可扩展性**

   * 业务控制面与模型推理面解耦
   * 支持后续替换 ASR 模型、声纹模型与云端服务
   * 支持从单机形态演进到私有化或集中式部署

### 2.2 非目标

当前阶段不作为重点处理的内容包括：

* 模型训练与微调体系
* 大规模分布式向量检索集群
* 多区域多活容灾设计
* 复杂权限平台、计费系统、运营后台

---

## 3. 总体设计原则

### 3.1 分层解耦

系统拆分为：

* **客户端层**：音频采集、结果展示、交互控制
* **控制层（Rust）**：会话管理、路由决策、结果聚合、业务逻辑
* **推理层（Python Worker）**：VAD、ASR、声纹特征提取、可选 diarization
* **数据层（PostgreSQL + pgvector）**：业务数据与声纹向量数据统一管理

### 3.2 本地优先，云端增强

默认优先使用本地推理，以保障：

* 隐私
* 实时性
* 成本可控
* 离线可用性

在以下场景引入云端能力：

* 设备性能不足
* 长音频高精度整理
* 多语种复杂场景
* 本地推理失败或超时

### 3.3 控制面与模型面分离

* Rust 负责控制流、状态、数据、策略与接口
* 模型推理能力通过 Python Worker subprocess 提供（gRPC 通信）
* 业务逻辑不直接依赖具体模型实现细节

### 3.4 以时间轴为主线组织结果

系统内部所有音频相关结果统一绑定时间区间：

* `start_ms`
* `end_ms`

ASR 结果、speaker 结果、分段结果围绕统一时间轴进行对齐与聚合。

---

## 4. 系统整体架构

## 4.1 逻辑架构

```text
[Client / Desktop / Web]
    ├─ 音频采集
    ├─ 控制指令
    └─ 结果展示
            |
            ▼ REST/HTTP + WebSocket
[Rust Core Service]
    ├─ API Gateway (REST + WS)
    ├─ Session Manager
    ├─ Audio Stream Coordinator
    ├─ Inference Router
    ├─ Speaker Matcher
    ├─ Transcript Aggregator
    ├─ Policy Engine
    └─ Repository Layer
            |
            | gRPC (内部通信)
            ▼
[Python Worker Process]
    ├─ VAD
    ├─ FunASR (Streaming + Offline ASR)
    ├─ CAM++ (Speaker Embedding Extraction)
    └─ 可选 Speaker Diarization
            |
            | Rust 控制匹配逻辑 → pgvector top-k 查询
            ▼
[PostgreSQL + pgvector]
    ├─ 业务数据 (Speaker, Session, Segment, Transcript)
    ├─ 声纹向量 (embedding_vector, dimension=192 for CAM++)
    ├─ 审计日志 (Verification, Routing)
    └─ 会话与分段数据
```

### 4.1.1 协议策略

**外部协议**（Client → Rust）：REST + WebSocket
- REST 用于控制操作（创建会话、注册说话人、查询结果、策略管理）
- WebSocket 用于实时音频流输入和转写结果输出

**内部协议**（Rust → Python Worker）：gRPC
- 双向流式 gRPC 用于音频流式识别
- 标准 gRPC 用于 one-shot 调用（声纹提取、说话人验证）

### 4.1.2 音频格式标准

系统内部统一使用：

* **采样率**：16kHz
* **位深**：16-bit PCM
* **通道**：Mono
* **编码**：Raw PCM / WAV
* **Chunk 大小**：可配置，默认 600ms（9600 samples）

客户端发送的音频将被转换为上述标准格式后进入内部流水线。

## 4.2 部署形态

系统支持三种逻辑形态：

### 形态 A：单机本地模式

* 本地客户端
* 本地 Rust 服务
* 本地 Python Worker（子进程）
* 本地 PostgreSQL

适合：

* 高隐私
* 离线办公
* 原型验证
* 单用户或小范围使用

### 形态 B：本地 + 云端混合模式

* 本地客户端
* 本地 Rust 服务 / 边缘服务
* 本地 Python Worker
* 云端 ASR 增强
* 集中式 PostgreSQL 或本地数据库

适合：

* 普通电脑覆盖
* 兼顾实时与精度
* 面向实际产品化

### 形态 C：企业私有化集中模式

* 客户端接入统一 Rust 网关
* 集中式 Python 推理服务
* 数据集中存储
* 可引入 GPU 节点

适合：

* 企业会议、客服、呼叫中心
* 内网私有部署
* 多用户统一管理

---

## 4.3 核心架构决策

### 架构选择：Rust + Python Bridge

**决策**：控制层用 Rust，模型推理用 Python Worker（通过 gRPC 调用）。

**理由**：
- FunASR 和 CAM++ 都是 Python-first 的模型库
- Python 生态有完整的模型加载、推理、调试工具链
- Rust ONNX Runtime 生态还不够成熟，模型调试困难
- 未来可以逐步迁移到纯 Rust（当模型 ONNX 支持成熟时）

**代价**：
- 多语言依赖
- Python Worker 需要独立的进程管理和健康检查
- 打包和部署稍复杂

---

## 5. 功能架构设计

## 5.1 功能域划分

系统分为五个核心功能域：

1. 音频接入域
2. ASR 域
3. 声纹域
4. 路由与策略域
5. 数据与结果管理域

---

## 5.2 音频接入域

### 目标

统一承接各种音频输入来源，并转化为系统内部标准音频流。

### 支持输入类型

* 麦克风实时音频
* 系统音频 / 会议音频
* 上传音频文件
* 第三方实时音频流

### 核心能力

* 建立音频会话
* 音频流收包与顺序管理
* chunk 切分与排序
* 音频格式标准化（16kHz/16-bit/mono/PCM）
* 会话状态维持
* 中断、重连与结束处理
* 背压控制（max buffer = 5s audio = 160KB per session）

### 输出

* 标准音频 chunk
* 会话上下文
* 音频源元数据

---

## 5.3 ASR 功能域

### 目标

将输入音频转换为带时间信息的文本结果。

### 子功能

#### 5.3.1 实时流式转写

适用于：

* 会议实时字幕
* 在线语音交互
* 边录边转写

输出：

* partial transcript（每 ~200ms）
* final transcript
* 时间区间

#### 5.3.2 离线文件转写

适用于：

* 录音文件整理
* 会后纪要
* 高精度文本生成

输出：

* 完整转写文本
* 分段文本
* 时间戳信息

#### 5.3.3 文本后处理

包括：

* 标点恢复
* 段落合并
* 术语纠正
* 重复内容清理
* 说话人标签回填

### ASR 功能输出结构

每一段转写结果至少包含：

* 会话标识
* 分段标识
* 起止时间
* 文本内容
* 是否最终结果
* 来源后端（本地 / 云端）

---

## 5.4 声纹功能域

### 目标

基于说话人声音特征，支持身份注册、匹配、检索与多人映射。

### 子功能

#### 5.4.1 说话人注册

用户提供若干注册样本，系统提取 speaker embedding 并建立 speaker profile。

输出：

* speaker profile
* embedding 集合
* 质量信息

#### 5.4.2 1:1 验证

输入一段待测音频与指定 speaker，判断是否为同一人。

输出：

* similarity score
* threshold
* accepted / rejected

#### 5.4.3 1:N 识别

输入一段待测音频，在声纹库中检索最相近的若干说话人。

输出：

* top-k speaker candidates
* 相似度分数
* 最终映射结果

#### 5.4.4 多人映射

针对会议类场景，对不同时间段内的说话人片段进行 embedding 提取与身份映射。

输出：

* segment -> speaker 映射
* speaker score
* unknown speaker 标记

#### 5.4.5 声纹管理

包括：

* speaker profile 管理
* embedding 增删改查
* 主模板 / 辅模板管理
* 声纹版本管理

#### 5.4.6 声纹阈值策略

CAM++ 在不同环境、设备、样本质量下固定阈值效果不一致。

**策略**：使用 Z-Norm 分数归一化
- 将相似度分数与数据库中已有说话人群集做归一化
- 不需要额外的基础设施
- 能处理大部分的环境差异
- 默认阈值：0.0（归一化后）

---

## 5.5 路由与策略域

### 目标

根据上下文动态决定系统使用哪种推理路径。

### 决策维度

* 是否允许云端
* 当前设备能力
* 是否要求离线
* 音频长度
* 当前本地推理负载
* 任务类型（实时 / 离线）
* 隐私级别
* 网络状态

### 决策结果

* 本地 ASR
* 云端 ASR
* 本地 speaker embedding
* 本地强制 speaker matching
* 本地失败回退云端
* 云端失败回退本地降级路径

### 策略范式

#### 默认策略

* VAD：本地
* speaker embedding / matching：本地
* 实时 ASR：本地优先
* 长音频 ASR：云端优先
* 高隐私任务：禁止上传音频

---

## 5.6 数据与结果管理域

### 目标

对整个语音会话生命周期中的元数据、结果数据、向量数据进行统一管理。

### 管理对象

* speaker profile
* speaker embeddings
* audio session
* audio segment
* transcript
* verification log
* routing log

### 核心能力

* 数据持久化
* 查询与检索
* 审计与追踪
* 按时间轴回放
* 结果二次聚合

---

## 6. 核心模块设计

## 6.1 Client

### 职责

* 音频采集
* 控制指令发送
* 会话开始 / 暂停 / 结束
* 实时结果显示
* 错误提示

### 输入输出

输入：

* 用户操作
* 麦克风 / 文件 / 音频流

输出：

* 标准音频 chunk
* 控制事件

---

## 6.2 API Gateway

### 职责

* 对外统一入口
* 鉴权（Bearer token）
* REST API：创建会话，注册说话人，查询结果，策略管理
* WebSocket：实时音频流输入 + 转写结果输出
* 路由请求
* 统一错误处理

---

## 6.3 Session Manager

### 职责

* 维护音频会话状态
* 管理会话上下文
* 跟踪 chunk 序号
* 管理 session 生命周期

### 核心状态

* session_id
* source_type
* mode
* routing_policy
* current_backend
* timestamps

---

## 6.4 Audio Stream Coordinator

### 职责

* 收取音频数据
* 对 chunk 做缓存、排序与切片
* 维持实时链路节奏
* 为推理层准备标准输入
* 背压控制：超过 5s buffer 时丢弃最早 chunk

---

## 6.5 Inference Router

### 职责

* 决定调用本地还是云端推理
* 按任务类型分发到对应后端
* 处理推理超时、失败与回退

### 输出

* 后端选择结果
* 路由原因
* 当前调用上下文

### 熔断器状态机 (Circuit Breaker)

```
STATE: Closed (local OK)
  ┌─ local fails ─▶ increment counter
  │   counter >= 3 ─▶ Open (use cloud for 30s)
  └─ local succeeds ─▶ reset counter

STATE: Open (using cloud)
  ┌─ cloud fails ─▶ close circuit, return error to client
  └─ 30s elapsed ─▶ Half-Open (try one local request)

STATE: Half-Open
  ┌─ local succeeds ─▶ Closed
  └─ local fails ─▶ Open (double timeout: 60s)
```

---

## 6.6 Python Worker Process (新增)

### 职责

作为独立的 Python 进程，承载模型推理能力：

* 管理 FunASR 与 CAM++ 模型生命周期
* 通过 gRPC 接收 Rust 请求
* 通过 gRPC 流式返回 ASR 结果
* 返回 speaker embedding 向量

### 不职责

* 业务逻辑
* 路由决策
* 数据库存取
* 结果聚合

这些由 Rust 控制层负责。

### 进程管理

* Rust 启动时自动拉起 Python Worker
* Rust 每 3s 发送 gRPC Health Check
* Worker 死亡后自动重启（最多 3 次，然后降级到云端）
* 模型加载失败时标记为 "cloud-only mode"

---

## 6.7 Local Inference Adapter (Rust gRPC Client)

### 职责

作为 Rust 控制层与 Python Worker 之间的 gRPC 客户端。

负责：

* 调用 VAD（gRPC unary）
* 调用 ASR 流式识别（gRPC bidirectional streaming）
* 调用 offline ASR（gRPC unary）
* 调用 speaker embedding 提取（gRPC unary）
* 调用 diarization（可选，gRPC unary）

### 不负责

* speaker 阈值判定
* speaker top-k 业务规则
* 数据库存储策略

这些应由 Rust 控制层负责。

---

## 6.8 Cloud ASR Adapter

### 职责

作为 Rust 控制层与云端转写能力之间的适配层。

### 功能

* 实时转写接入
* 离线文件转写提交
* 结果标准化
* 失败回退通知
* 熔断器集成

### 统一输出

将不同云服务返回的结果映射为统一 ASR 结果对象。

---

## 6.9 Speaker Matcher

### 职责

* 管理 speaker embedding 检索
* 实现 1:1 验证
* 实现 1:N 识别
* 管理阈值与置信度逻辑
* 将 segment 与 speaker profile 绑定

### 输入

* 当前音频 segment 的 embedding（来自 Python Worker）
* 待匹配 speaker 或可检索 speaker 范围

### 输出

* speaker_id
* score
* threshold
* accepted / rejected
* top-k 结果

### 匹配逻辑

* 余弦相似度计算在 PostgreSQL pgvector 中执行
* CAM++ embedding 维度固定为 192
* 若未来更换模型，需更新 `model_version` 和 `embedding_dimension`
* 模型不兼容的 embedding 将返回错误，不静默产生错误结果

---

## 6.10 Transcript Aggregator

### 职责

* 接收 partial / final transcript
* 统一按时间轴组织文本
* 合并碎片文本
* 对接 speaker 标签
* 生成结构化 transcript

### 延迟标签策略

* Transcript 立即输出，speaker 标签默认为 `[UNKNOWN]`
* Speaker 识别完成后（~2s 延迟），回溯更新该时间段的所有 transcript 标签
* 前端在 speaker 标签回填时实时更新显示

### 聚合目标

输出按以下维度组织的结果：

* 会话级 transcript
* speaker 级 transcript
* 时间段 transcript

---

## 6.11 Policy Engine

### 职责

* 解释用户配置与租户策略
* 控制云端可用性
* 控制隐私等级
* 控制自动回退行为
* 控制不同场景下的默认路径

### 示例策略

* `local_only`
* `allow_cloud_asr`
* `speaker_local_required`
* `long_audio_cloud_preferred`
* `privacy_level`

---

## 6.12 Repository Layer

### 职责

* 屏蔽具体数据库访问细节
* 提供 speaker / session / transcript / segment 的读写接口
* 支持向量相似度查询
* 支持日志追踪

---

## 7. 数据模型设计

## 7.1 核心实体

### Speaker

表示一个说话人主体。

属性包括：

* speaker_id
* tenant_id
* display_name
* status
* created_at

### SpeakerEmbedding

表示一个声纹向量模板。

属性包括：

* embedding_id
* speaker_id
* model_name
* model_version
* embedding_vector (pgvector, dimension: 192 for CAM++)
* duration_ms
* quality_score
* is_primary
* embedding_dimension (新增，与模型匹配)

### AudioSession

表示一次音频处理会话。

属性包括：

* session_id
* tenant_id
* source_type
* mode
* routing_policy (新增)
* started_at
* ended_at
* metadata

### AudioSegment

表示会话中的一个音频片段。

属性包括：

* segment_id
* session_id
* start_ms
* end_ms
* speaker_id
* speaker_score
* audio_ref
* quality_score
* vad_confidence (新增)

### TranscriptSegment

表示一个转写文本片段。

属性包括：

* transcript_segment_id
* session_id
* segment_id
* start_ms
* end_ms
* text
* is_final
* backend_type

### VerificationLog

表示一次 speaker 验证或识别决策记录。

属性包括：

* log_id
* session_id
* segment_id
* target_speaker_id
* score
* threshold
* decision
* created_at

### RoutingDecisionLog (新增)

记录每次路由决策的审计日志。

属性包括：

* log_id
* session_id
* timestamp
* decision (local / cloud / cloud_fallback / local_fallback)
* reason
* latency_ms

---

## 7.2 数据关系

* 一个 Speaker 对应多个 SpeakerEmbedding
* 一个 AudioSession 对应多个 AudioSegment
* 一个 AudioSegment 可对应零个或一个 speaker 映射结果
* 一个 AudioSegment 可对应多个 transcript 更新版本

---

## 7.3 缺失实体的处理

### User / Auth (待补充)

当前设计未包含用户模型。需要在 MVP 之后补充：
- User 实体
- APIKey / Token 模型
- 鉴权策略

### Tenant 实体

每个实体都有 tenant_id 但目前没有 Tenant 表。需要在多租户模式下补充。

### Config / Settings (待补充)

Policy Engine 的策略从哪里读取？需要一个配置存储表。

---

## 8. 数据流设计

## 8.1 实时转写 + speaker 识别

```text
Client 采集音频 (16kHz/16bit/mono)
    -> WebSocket 发送到 Rust
    -> Rust 接收 chunk, 排序, 缓存
    -> 本地 VAD (Python Worker) 切出有效 segment
    -> segment 送 Python Worker 提取 speaker embedding (CAM++)
    -> Rust 做 speaker top-k 匹配 (pgvector 查询)
    -> 同时送 Python Worker ASR (FunASR 流式)
    -> Transcript Aggregator 聚合文本
    -> 将 speaker 标签回填到文本段 (延迟 ~2s)
    -> WebSocket 推送结果到前端
```

## 8.2 长音频文件处理

```text
文件上传
    -> Rust 建立离线任务
    -> 预分析音频结构
    -> 若需要高精度则路由到云端 ASR
    -> speaker 相关在 Python Worker 完成
    -> 结果回写数据库
    -> 生成最终 transcript
```

## 8.3 说话人注册流程

```text
用户提交若干样本音频
    -> Python Worker 提取多个 embedding (CAM++)
    -> Rust 做质量校验
    -> 建立 speaker profile
    -> 写入 embedding 集合
```

## 8.4 1:1 speaker 验证流程

```text
输入待测音频 + 指定 speaker_id
    -> Python Worker 提取 embedding
    -> Rust 从库中读取目标 speaker embeddings
    -> 计算余弦相似度
    -> 基于 Z-Norm 阈值做接受 / 拒绝判定
    -> 写 verification log
```

## 8.5 1:N speaker 识别流程

```text
输入待测音频
    -> Python Worker 提取 embedding
    -> Rust 在 pgvector 中做 top-k 检索
    -> 基于 Z-Norm 分数与阈值选择 speaker
    -> 若无满足阈值的候选，则标记 unknown
```

## 8.6 影子路径 — 异常数据流

```
核心流:
  INPUT ──▶ VALIDATION ──▶ VAD ──▶ ASR + Speaker Embedding ──▶ Aggregate ──▶ Output
    │            │           │              │                        │            │
    ▼            ▼           ▼              ▼                        ▼            ▼
  [nil?]    [invalid?]   [model error]  [worker dead]         [timeout]     [client gone]
  [empty?]  [wrong type] [timeout]      [OOM/crash]           [partial]     [stale]
  [dup?]    [OOM?]        [silent]      [network error]       [corrupt]     [fallback]

各影子路径处理:
  nil/empty:     丢弃 + 日志
  wrong type:    返回错误 (BAD REQUEST)
  model error:   降级云端, 若不可用则返回错误
  worker dead:   自动重启 + 云端 fallback
  timeout:       5s 强制 final, 输出当前最佳
  OOM:           标记 cloud-only mode, 告警
  network error: 退避重试 3 次, 失败则降级
```

---

## 9. 本地 / 云端混合架构设计

## 9.1 混合目标

在以下目标之间取得平衡：

* 隐私
* 覆盖普通电脑
* 实时性
* 文本质量
* 运维复杂度
* 成本

## 9.2 能力分配原则

### 本地固定能力

优先本地执行：

* 音频采集
* VAD
* speaker embedding 提取
* speaker verification / identify
* 基础实时 ASR

### 云端增强能力

优先云端执行：

* 长音频高质量转写
* 高复杂度多语种场景
* 本地资源不足时的转写任务
* 本地失败任务回退

## 9.3 路由优先级

默认优先级：

1. 若策略强制本地，则走本地
2. 若本地能力不足且允许云端，则走云端
3. 若本地失败且允许回退，则回退云端
4. 若云端失败，则降级本地最小可用路径
5. **熔断器激活时**：直接使用云端，跳过本地尝试

## 9.4 隐私边界

建议将 speaker 相关能力尽量保留在本地或私有环境：

* speaker embedding
* speaker profile
* 匹配阈值逻辑
* speaker 历史记录

云端主要承担 ASR 增强职责。

---

## 10. 接口抽象设计

## 10.1 控制层内部统一抽象

为了支持未来替换推理后端，控制层应仅依赖抽象接口，而非具体厂商或模型。

### gRPC 服务定义 (Rust ↔ Python)

```protobuf
service VADService {
  rpc DetectSpeech(AudioStream) returns (stream VADResult);
}

service ASRService {
  rpc StreamRecognize(stream AudioChunk) returns (stream RecognizeResult);
}

service SpeakerService {
  rpc ExtractEmbedding(AudioSegment) returns (EmbeddingResponse);
  rpc VerifySpeaker(VerificationRequest) returns (VerificationResponse);
}

message AudioChunk {
  bytes audio_data = 1;       // 16kHz/16bit/mono PCM, ~600ms
  int64 timestamp_ms = 2;
  int32 sequence_num = 3;
  string session_id = 4;
}

message RecognizeResult {
  string text = 1;
  bool is_final = 2;
  int64 start_ms = 3;
  int64 end_ms = 4;
  string session_id = 5;
}

message EmbeddingResponse {
  repeated float embedding = 1;  // 192-dim for CAM++
  float quality_score = 2;
  float duration_ms = 3;
}
```

### ASR Backend (Rust trait)

统一抽象：

* 流式识别
* 文件识别
* 结果标准化

### Speaker Backend (Rust trait)

统一抽象：

* 提取 embedding
* 可选 verify/diarize
* 输出统一向量对象

### Storage Backend (Rust trait)

统一抽象：

* speaker 数据读写
* transcript 数据读写
* segment 数据读写
* 检索接口

---

## 11. 典型业务场景架构映射

## 11.1 单人桌面语音助手

特点：

* 单说话人
* 高隐私
* 实时字幕或命令识别

推荐路径：

* 本地 VAD
* 本地 ASR (Python Worker)
* 本地 speaker verify (CAM++)
* 禁止上传云端

## 11.2 企业会议纪要

特点：

* 多人
* 实时字幕 + 会后整理
* 需要 speaker 标签

推荐路径：

* 本地实时 ASR 输出草稿 (Python Worker)
* 本地 CAM++ speaker identify
* 会后长音频走云端高精度整理
* 结果回填 speaker 标签

## 11.3 客服 / 呼叫中心质检

特点：

* 高频长音频
* 已知角色集合
* 集中部署

推荐路径：

* 集中式 Rust 网关
* 集中式 Python Worker 推理服务
* 集中式 PostgreSQL / pgvector
* 长音频与批量任务优先云端或私有 GPU

---

## 12. 可扩展性设计

## 12.1 模型替换能力

系统应支持未来替换：

* 本地 ASR 模型
* speaker embedding 模型
* 云端 ASR 提供方
* diarization 方案

替换时只需修改推理适配层，不影响控制层与数据库层。

**注意**：模型替换会导致 embedding vector 的维度变化。所有 embedding 必须记录 `embedding_dimension`，Speaker Matcher 在比较前先检查维度兼容性。

## 12.2 存储升级能力

当前向量存储采用 PostgreSQL + pgvector。

当出现以下情况时，可演进为独立向量库：

* 向量规模迅速扩大
* 检索成为主瓶颈
* 需要更强的 ANN 专项能力

在此之前，使用 pgvector 统一关系数据与向量数据更有利于简化系统。

## 12.3 推理迁移能力

后续可逐步将稳定的轻量能力迁移为 Rust 原生推理：

优先迁移：

* VAD (Silero → ONNX → Rust via `ort` crate)
* Speaker embedding (CAM++ → ONNX → Rust via `ort` crate)

谨慎迁移：

* Streaming ASR (FunASR 的流式识别逻辑复杂，需要 Python 生态支持)

---

## 13. MVP 范围建议

## 13.1 第一阶段 MVP

优先实现：

1. 实时音频接入 (WebSocket → Rust)
2. gRPC 通信框架 (Rust ↔ Python Worker)
3. Python Worker 基础设施 (进程管理, 健康检查)
4. 本地 VAD (Python Worker 调用)
5. 本地 FunASR streaming ASR
6. 本地 CAM++ speaker embedding 提取
7. speaker 注册
8. 1:N speaker 识别 (pgvector top-k)
9. transcript + speaker 标签联动展示
10. PostgreSQL + pgvector 存储
11. 错误处理与熔断器
12. 结构化日志

### MVP 输出能力

* 单人 / 已知多人实时转写
* speaker 标签回填
* speaker 库注册与管理
* 本地优先
* Python Worker 健康自愈

## 13.2 第二阶段

增加：

* 长音频离线任务
* diarization
* 会后高精度整理稿
* 云端 ASR fallback
* 更多策略控制
* 更完整审计与日志
* 模型动态加载/卸载

## 13.3 第三阶段

增加：

* 企业多租户
* 集中式部署
* 更复杂权限
* 更丰富的云端与私有后端
* 用户认证模块

---

## 14. 主要架构风险

## 14.1 本地机型差异

不同电脑对本地 ASR 的支持差异较大，影响：

* 实时性
* 稳定性
* 用户体验

**Mitigation**：启动时做硬件能力检测（CPU 核数、可用内存、是否有 GPU），根据检测结果选择模型 tier。若检测到硬件不足，自动降级到云端模式。

## 14.2 speaker 阈值泛化问题

不同环境、设备、样本质量会导致固定阈值效果不一致。

**Mitigation**：使用 Z-Norm 分数归一化，以数据库中已有说话人样本为参照集做标准化。

## 14.3 文本与 speaker 标签错位

实时场景下 ASR 分段与 speaker 分段边界不完全一致，可能导致错位。

**Mitigation**：Transcript Aggregator 支持延迟 speaker 标签回填。Transcript 先以 `[UNKNOWN]` 标签输出，speaker 识别完成后再回溯更新。

## 14.4 云端回退策略复杂度

若本地与云端同时参与，需要明确：

* 谁是主结果
* 谁覆盖谁
* 如何避免重复和冲突

**Mitigation**：明确主次：本地是默认主结果，云端是回退。回退结果不自动覆盖本地结果，需要显式确认。

## 14.5 多人重叠讲话

当多人重叠讲话比例较高时，仅依靠普通分段和 speaker identify 可能不足，需要引入更强的 diarization 流程。

**Mitigation**：MVP 阶段不支持多人重叠。重叠时段标记为 `[OVERLAPPED]`，第二阶段引入 diarization 后再处理。

## 14.6 Python Worker 生命周期 (新增)

Python Worker 是新的故障域。如果 Worker 崩溃，整个本地推理能力不可用。

**Mitigation**：
- Rust 启动时自动拉起 Worker
- 每 3s 健康检查
- 崩溃后自动重启（最多 3 次）
- 超过重启限制后降级到云端
- 模型加载时检查内存是否充足

## 14.7 安全与隐私 (新增)

* Speaker embeddings 是生物识别 PII，存储时需要加密
* 音频数据包含敏感信息，传输必须加密
* 没有认证模型是一个 MVP 之后的必须补充项

---

## 15. 错误与异常处理

## 15.1 核心错误处理策略

每个失败点都必须：
- 定义异常类型
- 定义恢复动作
- 定义用户可见信息
- 定义日志级别
- 编写测试

## 15.2 错误处理矩阵

| 错误类型 | 检测方式 | 恢复动作 | 用户看到 | 日志级别 |
|----------|----------|----------|----------|----------|
| Python Worker 未启动 | gRPC 连接拒绝 | 自动拉起 Worker, 重试 3 次 | "服务启动中..." | WARN |
| Python Worker 崩溃 | 健康检查超时 6s | 自动重启, 会话切换到云端 | "重新连接中..." | ERROR |
| FunASR 模型加载失败 | 启动时检查 | 标记 cloud-only mode | "使用云端转写" | ERROR |
| CAM++ 模型加载失败 | 启动时检查 | 声纹功能禁用 | "声纹不可用" | ERROR |
| gRPC 超时 (>5s chunk) | 超时计数器 | 丢弃 chunk, 记录 | 短暂静音 | WARN |
| 云端 ASR 超时 (>30s) | 超时计数器 | 退避 60s, 标记本地降级 | 可能延迟 | ERROR |
| 云端 ASR 429 (限频) | HTTP 状态码 | 退避 60s, 本地 fallback | 可能降级 | WARN |
| 云端 ASR 401 (认证过期) | HTTP 状态码 | 刷新 token, 重试 | 透明 | ERROR |
| audio chunk 乱序 | sequence_num 检查 | 重排序, 延迟 200ms 缓冲 | 正常 | DEBUG |
| chunk buffer 溢出 | buffer > 5s audio | 丢弃最早 chunk | 可能丢音 | WARN |
| CAM++ 音频不足 (<2s) | segment 时长检查 | 标记 UNKNOWN | "[未知说话人]" | INFO |
| speaker top-k 空结果 | 无人注册 | 所有 segment 标记 UNKNOWN | "[未知说话人]" | INFO |
| 数据库连接池耗尽 | 连接等待 > 3s | 返回错误, 重试 | "服务不可用" | ERROR |
| pgvector 扩展未加载 | 启动时检查 | 阻止启动 | "启动失败" | CRITICAL |
| 客户端断开连接 | WebSocket 断开信号 | 保存临时结果, 清理资源 | --- | INFO |
| embedding 维度不匹配 | 读取时检查 | 返回错误, 不静默错误 | "重新注册声纹" | ERROR |

## 15.3 熔断器状态机

详见 6.5 Inference Router 中的熔断器定义。

---

## 16. 可观测性设计

## 16.1 结构化日志

每个跳点都要日志：
- chunk_in, vad_out, embedding_result
- asr_result, routing_decision
- speaker_match, transcript_emitted
- circuit_breaker_state_change, worker_health

## 16.2 核心指标

* active_sessions (实时)
* asr_latency_p50/p99
* cloud_usage_percent
* unknown_speaker_rate
* circuit_breaker_state
* python_worker_health
* embedding_count_per_speaker

## 16.3 路由审计日志

每次路由决策记录到 RoutingDecisionLog：
- session_id
- decision (local / cloud / cloud_fallback / local_fallback)
- reason
- latency_ms

## 16.4 Python Worker 健康

* 每 3s liveness probe
* 崩溃自动重启（最多 3 次）
* 模型加载状态报告

---

## 17. 最终架构结论

该系统建议采用 **本地优先、云端增强、控制面与推理面解耦** 的架构。

### 推荐架构主线

* **客户端**：负责音频采集与结果展示
* **Rust 控制层**：负责状态、路由、策略、聚合、存储
* **Python 推理层**：FunASR + CAM++ 模型推理（FunASR 为 ASR，CAM++ 为声纹）
* **云端推理层**：负责高精度或回退型 ASR
* **PostgreSQL + pgvector**：负责业务数据与 speaker 向量统一管理 (CAM++ 用 192-dim 向量)

### 架构价值

该方案能够在以下目标之间取得平衡：

* 支持大多数电脑
* 保持较好的隐私边界
* 保持足够的系统扩展性
* 便于从 MVP 演进为产品化系统

---

## 18. 附录：建议的模块边界总结

### Rust 控制层负责

* API Gateway
* Session 管理
* 音频协调
* 路由与策略
* 熔断器
* speaker 匹配业务逻辑
* transcript 聚合
* 数据库存取
* 审计与日志
* Python Worker 生命周期管理

### Python Worker 负责

* VAD
* FunASR (ASR)
* CAM++ (Speaker Embedding)
* diarization（可选）
* 模型加载与卸载

### 数据层负责

* speaker 数据
* embedding 数据 (192-dim for CAM++)
* session 数据
* segment 数据
* transcript 数据
* verification / routing 日志

### 云端负责

* 增强型 ASR
* 高质量离线转写
* 本地失败回退路径

---

## 19. 设计评审报告

### CEO Review Report (2026-04-05)

| 检查项 | 状态 | 发现 | 建议 |
|--------|------|------|------|
| 架构清晰度 | OK | 模块职责明确 | 采用 Rust + Python Bridge |
| 协议策略 | FIXED | 缺少协议定义 | 外部 REST+WS, 内部 gRPC |
| 错误处理 | FIXED | 19+ 异常点未定义 | 完整错误矩阵 + 熔断器 |
| 安全模型 | TODO | 无认证/无加密 | MVP 后补充 |
| 数据模型 | FIXED | 缺 3 个实体 | 补充 RoutingDecisionLog 等 |
| 音频标准 | FIXED | 未定义格式 | 16kHz/16-bit/mono/PCM |
| 可观测性 | FIXED | 无监控设计 | 补充日志、指标、审计日志 |
| 声纹阈值 | FIXED | 固定阈值不可行 | Z-Norm 归一化 |
| 数据流影子路径 | FIXED | 只有 happy path | 补充 8.6 异常流 |

### 总体评分：7.5 → 8.5/10 (审阅后改进)

---

## 20. 待办事项

- [ ] **用户认证模块** — MVP 后补充 User/APIKey/Token 模型
- [ ] **Tenant 实体** — 多租户模式下需要
- [ ] **Config/Settings 表** — Policy Engine 的读取源
- [ ] **模型分发策略** — 如何让用户安装 FunASR/CAM++ 模型
- [ ] **加密方案** — speaker embedding 和其他 PII 的加密存储

---

## 21. 一句话总结

这是一套以 **Rust 作为控制骨架、Python Worker 作为模型推理引擎 (FunASR + CAM++)、PostgreSQL/pgvector 作为统一数据底座、REST 外部 + gRPC 内部通信、以本地推理为默认路径、以云端 ASR 为增强与兜底** 的语音智能系统架构，适合从单机原型逐步演进到企业级产品。
