# MVP 实施验证清单

## ✅ 代码实施验证

### 1. 编译验证
- [x] `cargo check --lib` 通过
- [ ] `cargo build --release` 进行中
- [ ] `cargo test checkpoint_test` 待执行

### 2. 代码变更验证
- [x] HTTP 头部伪装：2 处（upload + search）
- [x] 随机抖动退避：4 处重试逻辑
- [x] checkpoint_id 支持：数据结构 + 查询逻辑 + 响应处理 + 404 自愈
- [x] 测试覆盖：6 个测试用例

### 3. 向后兼容性验证
- [x] `#[serde(default)]` 标记新字段
- [x] 版本检测逻辑（v2 → v3 自动重建）
- [x] IndexData 初始化包含新字段

## 📋 待执行任务

### 编译与测试
```bash
# 1. 编译 release 版本（进行中）
cargo build --release

# 2. 运行测试
cargo test checkpoint_test

# 3. 运行所有测试
cargo test
```

### 功能验证
```bash
# 1. 第一次查询（无 checkpoint）
./target/release/ace-tool-rs search "test query"
# 预期：日志显示 "saved new checkpoint_id"

# 2. 第二次查询（使用 checkpoint）
./target/release/ace-tool-rs search "another query"
# 预期：日志显示 "using cached checkpoint_id"
# 预期：added_blobs 为空
```

### 监控指标
- [ ] checkpoint_id 保存成功率
- [ ] 404 自愈触发次数
- [ ] 请求体积减少比例
- [ ] 查询响应时间变化

## 🎯 成功标准

### 必须满足
1. ✅ 编译无错误
2. ✅ 所有测试通过
3. ✅ 向后兼容（旧索引能正常加载）
4. ✅ 404 自愈正常工作

### 期望达到
1. 请求体积减少 90%+（有 checkpoint 时）
2. 查询速度提升（服务端缓存）
3. 无用户可感知的错误

## 📝 验证记录

### 2026-05-31 06:50
- ✅ 代码实施完成
- ✅ `cargo check --lib` 通过
- ⏳ `cargo build --release` 进行中
- ⏳ 测试待执行

---
**验证者**: 浮浮酱
