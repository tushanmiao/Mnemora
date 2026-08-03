/** Review 的最小类型边界；复习算法和持久化模型在功能实现阶段再加入。 */
export type ReviewRating = "again" | "hard" | "good" | "easy";
export type ReviewCardSourceKind = "note" | "literature" | "conversation";
export type ReviewCardState = "new" | "learning" | "review" | "relearning";
