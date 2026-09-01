## ADDED Requirements

### Requirement: Archetype 迁移后实体行号一致性
Archetype 迁移（add / remove / destroy 触发实体在 archetype 间换行）时，若移除行 `row` 后以 swap 方式将最后一行搬入 `row`，该被搬入实体的槽位行号 MUST 被更新为 `row`。经 swap 移除后，按实体句柄访问（`get` / `get_mut`）MUST 返回该实体自身的组件数据，且不越界、不 panic、不产生未定义行为；被移除行对应的实体组件数据 MUST 被正确 drop。迁移前后 `entity_count` MUST 保持不变（destroy 除外），且每个存活实体 MUST 仍能解析出自身组件。

#### Scenario: 批量 add/remove 组件后每实体 get 值正确
- **WHEN** 对 N 个实体批量 add 组件（触发从无该组件的 archetype 迁入），随后再对同批实体 remove 该组件（触发迁出）
- **THEN** 每个实体 `get::<T>` 返回自身组件值（与迁移前一致），无越界 / panic / 数据错位，`entity_count` 保持 N

#### Scenario: 批量 destroy 后其余实体可访问且值正确
- **WHEN** 对含多个实体的 archetype 批量 destroy 其中若干实体（触发 swap 搬入）
- **THEN** 其余未销毁实体仍可通过句柄解析出自身组件且值正确，无越界 / panic

#### Scenario: 重复 toggle（往返迁移）后句柄与数据一致
- **WHEN** 对同一批实体反复 add/remove 同一组件（往返迁移）多轮
- **THEN** 每轮后每个实体 `get::<T>` 返回自身原值，`entity_count` 恒定，含该组件的实体数正确；整个过程无 panic / 越界
