//! The wizard's shape: a list of fields, what kind each one is, and how the
//! prompt steps through them.

use super::*;

/// What the wizard is creating. A **Spoke** is a road-warrior ewg generates a key
/// for; it dials the hub(s) you pick. A **Hub** is reachable (has an endpoint) and
/// meshes with every other hub; its key may live elsewhere (referenced by pubkey).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum NodeKind {
    Spoke,
    Hub,
}

impl NodeKind {
    pub(super) fn short(self) -> &'static str {
        match self {
            NodeKind::Spoke => "Spoke",
            NodeKind::Hub => "Hub",
        }
    }
    pub(super) fn toggled(self) -> Self {
        match self {
            NodeKind::Spoke => NodeKind::Hub,
            NodeKind::Hub => NodeKind::Spoke,
        }
    }
}

/// Where a node's key comes from: ewg generates a keypair, or you paste an existing
/// public key (its private lives elsewhere - e.g. a router's, vaulted in Ansible).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum KeySource {
    Generate,
    Paste,
}

impl KeySource {
    pub(super) fn short(self) -> &'static str {
        match self {
            KeySource::Generate => "Generate",
            KeySource::Paste => "Paste pubkey",
        }
    }
    pub(super) fn toggled(self) -> Self {
        match self {
            KeySource::Generate => KeySource::Paste,
            KeySource::Paste => KeySource::Generate,
        }
    }
}

/// A `Text` field is typed into; a `Type`/`Key` toggle or a `Pick` is a choice
/// flipped with ←/→ (nothing to type there). The toggles rebuild the form (their
/// choice changes which fields follow); a `Pick` just steps its index.
pub(super) enum FieldKind {
    Text,
    Type(NodeKind),
    Key(KeySource),
    Pick { options: Vec<String>, idx: usize },
}

/// One line in a wizard: a label in the left column, a value in the right. The
/// `default` is what a blank field submits, and stands in as the dim example
/// until something is typed.
pub(super) struct Field {
    pub(super) label: String,
    pub(super) default: String,
    /// The dim example shown in the value column while the field is empty and
    /// has no default: what the field wants, not what it is called. It lives
    /// here rather than in the label so every label stays one short noun and
    /// the values all start in the same column.
    pub(super) hint: String,
    pub(super) value: String,
    pub(super) kind: FieldKind,
    /// Marked with a red `*`, the form convention everyone already reads. Only
    /// set it on a field the submit path actually refuses to go without, or the
    /// star is a lie: everything unmarked can be left blank.
    pub(super) required: bool,
}

impl Field {
    pub(super) fn new(label: &str, default: &str) -> Self {
        Self {
            label: label.into(),
            default: default.into(),
            hint: String::new(),
            value: String::new(),
            kind: FieldKind::Text,
            required: false,
        }
    }

    /// Builder: the submit path refuses a blank here, so it gets the red `*`.
    pub(super) fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Builder: the dim example shown while the field is empty. On a `Pick` it
    /// is what stands in when there is nothing to choose from yet.
    pub(super) fn hint(mut self, hint: &str) -> Self {
        self.hint = hint.into();
        self
    }

    /// What stands in the value column while the field is empty: what a blank
    /// submits, then what it wants typed. Both are dim, and both are gone the
    /// moment there is a value, so neither can be mistaken for one.
    pub(super) fn placeholder(&self) -> String {
        [self.default.as_str(), self.hint.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("  ")
    }
    pub(super) fn type_toggle(kind: NodeKind) -> Self {
        Self {
            label: "Type".into(),
            default: String::new(),
            hint: String::new(),
            value: String::new(),
            kind: FieldKind::Type(kind),
            required: false,
        }
    }
    pub(super) fn key_toggle(src: KeySource) -> Self {
        Self {
            label: "Key".into(),
            default: String::new(),
            hint: String::new(),
            value: String::new(),
            kind: FieldKind::Key(src),
            required: false,
        }
    }
    pub(super) fn pick(label: &str, options: Vec<String>) -> Self {
        Self {
            label: label.into(),
            default: String::new(),
            hint: String::new(),
            value: String::new(),
            kind: FieldKind::Pick { options, idx: 0 },
            required: false,
        }
    }
    /// A choice field (Type/Key toggle or Pick): ←/→ cycles it, letters don't type.
    pub(super) fn is_choice(&self) -> bool {
        matches!(
            self.kind,
            FieldKind::Type(_) | FieldKind::Key(_) | FieldKind::Pick { .. }
        )
    }
}

/// A modal wizard: a titled stack of fields plus the action to run on submit.
pub(super) struct Prompt {
    pub(super) title: String,
    pub(super) fields: Vec<Field>,
    pub(super) idx: usize,
    pub(super) action: Action,
}

impl Prompt {
    pub(super) fn cur_mut(&mut self) -> &mut Field {
        &mut self.fields[self.idx]
    }

    /// The node kind (read off the Type toggle row).
    pub(super) fn kind(&self) -> NodeKind {
        match self.fields.first().map(|f| &f.kind) {
            Some(FieldKind::Type(k)) => *k,
            _ => NodeKind::Spoke,
        }
    }

    /// The key source (read off the Key toggle row, if any; else Generate).
    pub(super) fn keysrc(&self) -> KeySource {
        self.fields
            .iter()
            .find_map(|f| {
                if let FieldKind::Key(k) = &f.kind {
                    Some(*k)
                } else {
                    None
                }
            })
            .unwrap_or(KeySource::Generate)
    }

    /// Look up a field's value by label prefix: a Pick's selected option, or a
    /// Text field's typed value (falling back to its default when blank).
    pub(super) fn value_of(&self, label_starts: &str) -> String {
        self.fields
            .iter()
            .find(|f| f.label.starts_with(label_starts))
            .map(|f| match &f.kind {
                FieldKind::Pick { options, idx } => options.get(*idx).cloned().unwrap_or_default(),
                _ => {
                    let v = f.value.trim();
                    if v.is_empty() {
                        f.default.clone()
                    } else {
                        v.to_string()
                    }
                }
            })
            .unwrap_or_default()
    }

    /// The metadata fields (everything except key material) - shared by create/edit.
    pub(super) fn base_fields(kind: NodeKind, default_addr: &str, hubs: &[String]) -> Vec<Field> {
        let mut fields = vec![
            Field::type_toggle(kind),
            Field::new("Name", "")
                .required()
                .hint("e.g. phone, laptop, vps"),
            Field::new("Address", default_addr),
        ];
        match kind {
            NodeKind::Spoke => {
                fields.push(Field::new("DNS", "").hint("e.g. a Pi-hole"));
                fields.push(
                    Field::pick("Hub to dial", hubs.to_vec())
                        .hint("no hubs yet - create one first"),
                );
            }
            NodeKind::Hub => {
                fields.push(
                    Field::new("Endpoint", "")
                        .required()
                        .hint("host:port peers dial"),
                );
                fields.push(Field::new("Allowed-IPs", "0.0.0.0/0"));
                fields.push(Field::new("Keepalive", "").hint("seconds, e.g. 25"));
            }
        }
        fields
    }

    /// The key-source toggle plus its one dependent field: generate -> store/redact
    /// the private; paste -> the existing public key to reference.
    pub(super) fn key_fields(keysrc: KeySource) -> Vec<Field> {
        let mut fields = vec![Field::key_toggle(keysrc)];
        match keysrc {
            KeySource::Generate => fields.push(Field::pick(
                "Private key",
                vec!["store in mesh.toml".into(), "redact (QR/file only)".into()],
            )),
            KeySource::Paste => fields.push(
                Field::new("Public key", "")
                    .required()
                    .hint("paste an existing pubkey"),
            ),
        }
        fields
    }

    pub(super) fn create_node(
        kind: NodeKind,
        keysrc: KeySource,
        default_addr: &str,
        hubs: &[String],
    ) -> Self {
        let mut fields = Self::base_fields(kind, default_addr, hubs);
        fields.extend(Self::key_fields(keysrc));
        Self {
            title: "create a mesh node".into(),
            idx: 0,
            action: Action::AddNode,
            fields,
        }
    }

    /// Prefill a Text field's value by label prefix (no-op if absent).
    pub(super) fn set(&mut self, label_starts: &str, value: &str) {
        if let Some(f) = self
            .fields
            .iter_mut()
            .find(|f| f.label.starts_with(label_starts))
        {
            f.value = value.to_string();
        }
    }

    /// Point a Pick field at `selected` if it's among the options.
    pub(super) fn set_pick(&mut self, label_starts: &str, selected: &str) {
        if let Some(f) = self
            .fields
            .iter_mut()
            .find(|f| f.label.starts_with(label_starts))
            && let FieldKind::Pick { options, idx } = &mut f.kind
            && let Some(pos) = options.iter().position(|o| o == selected)
        {
            *idx = pos;
        }
    }

    /// A wizard prefilled from an existing node. Edit changes metadata only - the
    /// key is untouched (rotate is how you change keys), so no key fields here.
    pub(super) fn edit_node(node: &Node, hubs: &[String]) -> Self {
        let kind = if node.endpoint.is_some() {
            NodeKind::Hub
        } else {
            NodeKind::Spoke
        };
        let mut p = Self {
            title: format!("edit `{}`", node.name),
            fields: Self::base_fields(kind, &node.address, hubs),
            idx: 0,
            action: Action::EditNode {
                original: node.name.clone(),
            },
        };
        p.set("Name", &node.name);
        p.set("Address", &node.address);
        p.set("DNS", node.dns.as_deref().unwrap_or(""));
        p.set("Endpoint", node.endpoint.as_deref().unwrap_or(""));
        p.set("Allowed-IPs", node.allowed_ips.as_deref().unwrap_or(""));
        p.set(
            "Keepalive",
            &node.keepalive.map(|k| k.to_string()).unwrap_or_default(),
        );
        if let Some(h) = node.hubs.first() {
            p.set_pick("Hub to dial", h);
        }
        p
    }

    /// Rows the field block takes: one per field, plus the blank that sets the
    /// key material off from the metadata, and never fewer than the tallest
    /// shape this wizard can rebuild into - a Hub carries one field more than a
    /// Spoke, and a frame that grows under the cursor reads as ←/→ having done
    /// something to the box rather than to the toggle.
    ///
    /// The floor is measured off the constructors rather than counted by hand,
    /// so a field added to either shape cannot leave this number behind.
    pub(super) fn body_rows(&self) -> usize {
        let keyed = self
            .fields
            .iter()
            .any(|f| matches!(f.kind, FieldKind::Key(_)));
        let floor = match self.action {
            Action::AddNode | Action::EditNode { .. } => {
                Self::base_fields(NodeKind::Hub, "", &[]).len()
                    + if keyed {
                        Self::key_fields(KeySource::Generate).len()
                    } else {
                        0
                    }
            }
            _ => 0,
        };
        floor.max(self.fields.len()) + usize::from(keyed)
    }

    /// Cycle the current Pick field by `delta` (no-op on other field kinds).
    pub(super) fn cycle_pick(&mut self, delta: isize) {
        if let FieldKind::Pick { options, idx } = &mut self.fields[self.idx].kind
            && !options.is_empty()
        {
            let n = options.len() as isize;
            *idx = (((*idx as isize + delta) % n + n) % n) as usize;
        }
    }

    /// Rebuild the form for a new kind/keysrc, carrying over what the user typed.
    pub(super) fn rebuild(
        &mut self,
        kind: NodeKind,
        keysrc: KeySource,
        default_addr: &str,
        hubs: &[String],
    ) {
        let old = std::mem::take(&mut self.fields);
        let mut next = if matches!(self.action, Action::EditNode { .. }) {
            Self::base_fields(kind, default_addr, hubs)
        } else {
            let mut f = Self::base_fields(kind, default_addr, hubs);
            f.extend(Self::key_fields(keysrc));
            f
        };
        for nf in next.iter_mut() {
            if let Some(of) = old.iter().find(|f| f.label == nf.label) {
                nf.value = of.value.clone();
                if let (
                    FieldKind::Pick { options, idx },
                    FieldKind::Pick {
                        options: oo,
                        idx: oi,
                    },
                ) = (&mut nf.kind, &of.kind)
                    && let Some(pos) = oo
                        .get(*oi)
                        .and_then(|sel| options.iter().position(|o| o == sel))
                {
                    *idx = pos;
                }
            }
        }
        self.fields = next;
        self.idx = self.idx.min(self.fields.len().saturating_sub(1));
    }

    /// Flip Spoke<->Hub and rebuild, landing back on the Type row.
    pub(super) fn toggle_kind(&mut self, default_addr: &str, hubs: &[String]) {
        self.rebuild(self.kind().toggled(), self.keysrc(), default_addr, hubs);
        self.idx = 0;
    }

    /// Flip Generate<->Paste and rebuild, staying on the Key row.
    pub(super) fn toggle_keysrc(&mut self, default_addr: &str, hubs: &[String]) {
        let at = self.idx;
        self.rebuild(self.kind(), self.keysrc().toggled(), default_addr, hubs);
        self.idx = at.min(self.fields.len().saturating_sub(1));
    }
}
