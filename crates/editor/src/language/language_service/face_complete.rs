//! 补全面（complete）：转发 crate::completion
//!
//! 自 language/language_service.rs 拆分（2026-10-29 Step 9）：按能力域切分。


use crate::completion::{self, Candidate, Catalog};
use super::*;
use super::face_actions::clamp_byte;

/// "未绑定连接就不摆元数据候选"这条口径钉在纯函数层。
pub fn complete(request: &Request) -> Vec<Candidate> {
    let text = request.text;
    let offset = clamp_byte(text, request.cursor);
    let ask = completion::request_at(text, offset);

    // 未绑定连接 = 没有目标库：目录里那部分一律不采用。宿主的端口实现本来就按这条口径
    // 返回空目录（见 `workbench/src/services/editor_completion.rs`），在这里再钉一次是让
    // "没有目标库就不摆元数据候选"这件事**在纯函数层可测**，而不是只活在端口契约的注释里。
    let empty = Catalog::default();
    let catalog = match request.connection {
        Some(_) => request.catalog,
        None => &empty,
    };

    completion::candidates(catalog, &ask, MAX_CANDIDATES)
}

// ---------------------------------------------------------------------------
// 面二：诊断（词法层判得动的那些）
// ---------------------------------------------------------------------------
