// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Simplified Chinese catalog. Account sentences match the previous screen copy.

use super::Key;

pub(crate) fn template(key: Key, _category: super::Category) -> &'static str {
    account(key)
        .or_else(|| chrome(key))
        .or_else(|| lines(key))
        .or_else(|| status(key))
        .or_else(|| cards(key))
        .or_else(|| parse_keys(key))
        .or_else(|| tokens(key))
        .unwrap_or_else(|| plural(key))
}

fn account(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::ChooseAccount => "选择账户",
        Key::ChooseAccountHint => "上下键选择。回车打开。Esc 退出。",
        Key::UnlockHint => "回车解锁。Esc 清除口令。",
        Key::HintKeys => "L 锁定  N 新建  P 修改口令  Y 恢复  G 语言",
        Key::HintConfirm => "回车确认。Esc 取消。",
        Key::HintHidePhrase => "请抄下这些词。回车后隐藏。",
        Key::UnlockTitle | Key::RecoverCodeTitle => "账户口令",
        Key::CreateNameTitle => "显示名称",
        Key::CreatePhraseTitle | Key::RecoverPhraseTitle => "恢复短语",
        Key::CreateCodeTitle | Key::RotateNewTitle => "新账户口令",
        Key::CreateRepeatTitle => "再次输入账户口令",
        Key::RotateOldTitle => "当前账户口令",
        Key::RotateRepeatTitle => "再次输入新账户口令",
        Key::RecoverAnchorTitle => "输入账户指纹",
        Key::WarningLabel => "警告",
        Key::DigitsOnly => "账户口令只能是数字。",
        Key::NotSixDigits => "账户口令必须是 6 位数字。",
        Key::TrivialCode => "这个口令太容易猜到。请另选一个。",
        Key::CodeMismatch => "两次输入的账户口令不一致。",
        Key::NameRequired => "需要显示名称。",
        Key::NameTooLong => "显示名称太长。",
        Key::WordRequired => "请输入恢复词。",
        Key::AnchorMismatch => "指纹不一致。",
        Key::MissingSocket => "这个界面需要守护进程套接字。",
        Key::DaemonRefused => "守护进程拒绝了账户请求。",
        Key::Locked => "账户已锁定。请输入账户口令。",
        Key::Unlocked => "账户已解锁。",
        Key::Created => "账户已创建。",
        Key::Rotated => "账户口令已更改。",
        Key::Recovered => "账户已恢复。",
        Key::Cancelled => "已取消。",
        Key::WordPrompt => "恢复词 {position}",
        Key::PhraseLine => "{position}. {word}",
        Key::RecoverWarning => "{label}\n{warning}\n\n{anchor}",
        _ => return None,
    })
}

fn chrome(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::About => "通过开放的本地 SDK 总线使用的私密 Ramflux 终端",
        Key::LangHelp => "界面语言：en 或 zh-CN",
        Key::Version => "{version}（构建 {build}）",
        Key::SdkError => "SDK 错误：",
        Key::JsonError => "JSON 错误：",
        Key::ReadModelError => "读取模型：",
        Key::TitleBar => "rf tui  {hints}",
        Key::InputCompose => "消息输入 [撰写]",
        Key::InputNormal => "消息输入 [普通 i=撰写]",
        Key::PanelConversations => "会话",
        Key::PanelMessages => "消息",
        Key::PanelObjects => "对象",
        Key::PanelContacts => "联系人",
        Key::PanelGroups => "群组",
        Key::PanelApprovals => "审批",
        Key::MessagesTitle => "消息  c=卡片  /attach <路径> l=已送达 r=已读 x=删除",
        Key::ObjectsTitle => "对象  i=输入 put/get/status/resume 命令  s=状态 r=继续",
        Key::ContactsTitle => "联系人/设备  i=输入 add/switch/accept 后回车  S=安全码 V=验证",
        Key::GroupsTitle => "群组  m=静音 u=取消静音 e=阅后即焚",
        Key::ApprovalsTitle => "审批  a=批准 d=拒绝",
        Key::CardChromeTitle => "卡片  ↑↓=选择 回车=执行 [ ]=卡片 Esc=返回",
        Key::Divider => "────────────────",
        _ => return None,
    })
}

fn lines(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::UnreadSuffix => "  未读={count} {status}",
        Key::MessageLine => "{marker}{id} {sender}：{body}（{status}）{suffix}",
        Key::ObjectLine => "{marker}{id} {direction} {state} {done}/{total} {percent}%{error}",
        Key::ObjectError => " 错误={value}",
        Key::AttachmentsSuffix => " 附件=[{summary}]",
        Key::ReceiptsSuffix => " 回执=[{summary}]",
        Key::ContactLine => {
            "{marker}{link} {requester} -> {target} {state} 验证={verification}\n  指纹={fingerprint}\n  安全码={safety}"
        }
        Key::DeviceLine => "{marker}设备 {id} 纪元={epoch} 目标={target}",
        Key::GroupLine => "{marker}{id} 成员={members} 阅后即焚={disappearing} 静音至={mute}",
        Key::ApprovalLine => "{marker}{id} {tool} 风险={risk} 模式={mode} {status} {hint}",
        Key::ApprovalApprove => "a=批准",
        Key::ApprovalRemoteHint => "remote_app: 需 App 签名",
        Key::CardSummary => "卡片：{fallback}  c=打开",
        Key::CardHeader => "卡片 {index}/{total} — {fallback}",
        Key::EditLine => "编辑：{text}  回车=保存 Esc=取消",
        Key::ButtonChrome => "[ {label} ]",
        Key::LinkUnavailable => "[{label}：此终端不支持链接或函数]",
        Key::UnsupportedComponent => "[不支持的卡片组件：{kind}]",
        Key::UnsupportedValue => "[不支持的显示值]",
        Key::ChoicesUnavailable => "[选项不可用]",
        Key::RepeatedUnavailable => "[此终端不支持重复内容]",
        Key::ChoiceItem => "{index}. {mark} {label}",
        Key::FieldLine => "{label}：{value}",
        _ => return None,
    })
}

fn status(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::StatusError => "错误：{detail}",
        Key::NoCards => "此会话没有卡片",
        Key::NoApproval => "未选择审批",
        Key::RemoteAppApproval => "该审批需 App 端签名授权(remote_app)",
        Key::NothingToSend => "没有可发送的内容",
        Key::AttachRefused => "conversation.send 不接受附件字节；请使用对象面板",
        Key::AttachmentNotSent => "排队的附件字节未发送；请打开对象面板发送文件",
        Key::OpIdTooLong => "operation_id 超过 256 字节",
        Key::EpochMissing => "撤销回执中缺少已签名的设备集纪元",
        Key::RevokeMismatch => "设备撤销回执不一致",
        Key::RevocationSubmitted => "已提交对 {device} 的撤销（已签名设备集纪元 {epoch}）",
        Key::ObjectUsage => {
            "对象命令：put <路径> <object_id> | get <object_id> <输出> | status <object_id> [方向] | resume <object_id> <方向>"
        }
        Key::DownloadedObject => "已下载对象 {object_id}",
        Key::NoTransfer => "未选择对象传输",
        Key::MissingRelay => "所选传输缺少中继端点",
        Key::NoContact => "未选择联系人",
        Key::BlindVerify => "拒绝盲目验证：请按 S 载入安全码，在带外核对全部 60 位数字后再验证",
        Key::ContactVerified => "联系人已验证",
        Key::UnknownCommand => "未知命令：{command}",
        Key::NoCommand => "未输入命令",
        Key::ContactAdded => "已添加联系人：{link_id}",
        Key::ContactAccepted => "已接受联系人：{link_id}",
        Key::ActiveAccount => "当前账户：{account}",
        Key::NoMessage => "未选择消息",
        Key::NoGroup => "未选择群组",
        Key::DefaultDm => "默认私聊",
        Key::ChangeBatchLimit => "消息变更批次已达到轮次上限并停止",
        Key::ChangeBatchWhat => "变更批次",
        Key::ConversationMismatch => "conversation_id 不一致",
        Key::MissingRelayEnv => "缺少中继对象传输所需的 {name}",
        Key::MissingArg => "缺少 {label}",
        Key::UnexpectedArg => "意外的 {command} 参数：{extra}",
        Key::StreamRefreshFailed => "流刷新失败：{error}",
        Key::Reconnecting => "正在重连：{error}",
        Key::SchemaReset => "必须重置消息模式后，此客户端才能继续。",
        _ => return None,
    })
}

fn cards(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::ContactAddLink => "contact add link_id",
        Key::ContactAddRequester => "contact add requester commitment",
        Key::ContactAddTarget => "contact add target commitment",
        Key::ContactAddCommand => "contact add",
        Key::ContactAcceptLink => "contact accept link_id",
        Key::ContactAcceptRequester => "contact accept requester commitment",
        Key::ContactAcceptTarget => "contact accept target commitment",
        Key::ContactAcceptCommand => "contact accept",
        Key::AccountSwitchId => "account switch account_id",
        Key::AccountSwitchCommand => "account switch",
        Key::EnterWholeNumber => "请输入整数",
        Key::MissingOptions => "缺少选项",
        Key::EnterOptionNumbers => "请输入以逗号分隔的选项编号",
        Key::OptionOutOfRange => "选项编号超出范围",
        Key::ChooseOne => "请只选择一项",
        Key::InputBindingUnavailable => "输入绑定不可用",
        Key::CardActionsUnavailable => "卡片操作不可用",
        Key::CardStateUnavailable => "卡片状态不可用",
        Key::CardSnapshotUnavailable => "卡片快照不可用",
        Key::ButtonNoEvent => "按钮没有事件",
        Key::UnsupportedContext => "不支持的操作上下文",
        Key::MissingCardHash => "缺少卡片散列",
        Key::ControlUnavailable => "控件不可用",
        Key::CheckboxUnavailable => "复选框的值不可用",
        Key::ChoiceHint => "按显示顺序输入选项编号（1,2）；留空则清除选择",
        Key::CardChanged => "卡片已变化；请再次查看后再操作",
        Key::ActionSubmitted => "操作已提交；正在等待代理回执",
        Key::ActionRefused => "操作被拒绝：{reason}",
        Key::NotAccepted => "未被接受",
        Key::ActionSendFailed => "操作发送失败；送达情况未知。请刷新后再操作",
        Key::CardPrefixed => "卡片：{detail}",
        Key::SurfaceMissing => "缺少 {key}",
        Key::SurfaceScopeMismatch => "界面范围不一致",
        Key::CardUnavailable => "卡片不可用",
        Key::TextFallback => "文本回退：此卡片不是来自你自己的代理",
        Key::CardContentUnavailable => "卡片内容不可用",
        Key::MissingSnapshot => "缺少快照",
        Key::SnapshotIdentity => "快照身份不一致",
        Key::InvalidSnapshot => "快照无效",
        Key::MissingVersion => "缺少版本",
        Key::SnapshotHash => "快照散列不一致",
        Key::CardClosed => "卡片已关闭",
        Key::CardOutOfSync => "卡片不同步；操作不可用",
        Key::ReceiptVerified => "操作回执已验证",
        Key::ReceiptPending => "操作已记录；回执待处理",
        Key::CardsLoadFailed => "卡片未能载入；按 c 重试",
        Key::InvalidSurfaceList => "界面列表无效",
        Key::SurfacePageTooLarge => "界面页过大",
        Key::DuplicateSurface => "界面引用重复",
        Key::CursorDidNotAdvance => "界面游标没有前进",
        _ => return None,
    })
}

fn parse_keys(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::CursorSequenceString => "next_before 的设备序列必须是字符串",
        Key::SchemaMismatch => "模式不是 ramflux.message.read_model.v1",
        Key::MustBeObject => "{what} 必须是对象",
        Key::RetiredField => "已停用字段 {key}",
        Key::MissingField => "缺少字段 {key}",
        Key::MustBeString => "{key} 必须是字符串",
        Key::MustBeInteger => "{key} 必须是整数",
        Key::MustFit32 => "{key} 必须能放入 32 位",
        Key::MustBeBoolean => "{key} 必须是布尔值",
        Key::MessagesMustBeArray => "{what} 的消息必须是数组",
        Key::TooManyMessages => "{what} 的消息超过 100 条",
        Key::DeviceSequenceNotNumber => "device_sequence 必须是字符串，不能是数字",
        Key::DeviceSequenceDigits => "device_sequence 必须是 20 位十进制字符串",
        Key::GroupIdString => "group_id 必须是字符串",
        Key::ContentError => "content：{error}",
        Key::ReactionsArray => "reactions 必须是数组",
        Key::ReceiptsArray => "receipts 必须是数组",
        Key::HlcMsLimit => "hlc_ms 必须小于 2^53",
        Key::HlcLogicalLimit => "hlc_logical 必须能放入 16 位",
        Key::EmptyIdentity => "消息身份为空",
        Key::BadMessageState => "消息状态不是 content、unsupported 或 tombstone",
        Key::UnsupportedMessage => "此消息需要更新的版本",
        _ => return None,
    })
}

fn tokens(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::BoolTrue => "是",
        Key::BoolFalse => "否",
        Key::ButtonLabel => "按钮",
        Key::SenderMe => "我",
        Key::StatusSending => "发送中",
        Key::StatusSynced => "已同步",
        Key::StatusArchived => "已归档",
        Key::StatusReferenced => "已引用",
        Key::StatusPending => "待处理",
        Key::StatusUnknown => "未知",
        Key::StatusUnverified => "未验证",
        Key::StatusVerified => "已验证",
        Key::StatusDeleted => "已删除",
        Key::StatusDelivered => "已送达",
        Key::StatusRead => "已读",
        Key::StatusContent => "内容",
        Key::StatusUnsupported => "不支持",
        Key::StatusTombstone => "已擦除",
        Key::TransferInProgress => "进行中",
        Key::TransferComplete => "完成",
        Key::DirectionUpload => "上传",
        Key::DirectionDownload => "下载",
        Key::NoneValue => "无",
        Key::StatusAccepted => "已接受",
        _ => return None,
    })
}

fn plural(key: Key) -> &'static str {
    match key {
        Key::FirstCards => "本会话仅显示前 {count} 张卡片",
        Key::QueuedAttachments => "排队附件数={count}",
        _ => "",
    }
}
