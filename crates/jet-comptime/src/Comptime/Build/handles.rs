#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolchainId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SigningIdentityId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProbeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GeneratedModuleId(pub usize);

macro_rules! target_handle {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name {
            pub(super) id: TargetId,
            pub(super) context: u64,
        }

        impl $name {
            pub fn id(self) -> TargetId {
                self.id
            }
        }

        impl From<$name> for TargetRef {
            fn from(value: $name) -> TargetRef {
                TargetRef {
                    id: value.id,
                    context: value.context,
                }
            }
        }
    };
}

target_handle!(ExecutableTarget);
target_handle!(LibraryTarget);
target_handle!(TestTarget);
target_handle!(BenchTarget);
target_handle!(AssetBundleTarget);
target_handle!(DocTarget);
target_handle!(InstallTarget);
target_handle!(PackageTarget);
target_handle!(PublishTarget);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetRef {
    pub(super) id: TargetId,
    pub(super) context: u64,
}

impl TargetRef {
    pub fn id(self) -> TargetId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionHandle {
    pub(super) id: ActionId,
    pub(super) context: u64,
}

impl ActionHandle {
    pub fn id(self) -> ActionId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolchainHandle {
    pub(super) id: ToolchainId,
    pub(super) context: u64,
}

impl ToolchainHandle {
    pub fn id(self) -> ToolchainId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SigningIdentityHandle {
    pub(super) id: SigningIdentityId,
    pub(super) context: u64,
}

impl SigningIdentityHandle {
    pub fn id(self) -> SigningIdentityId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProbeHandle {
    pub(super) id: ProbeId,
    pub(super) context: u64,
}

impl ProbeHandle {
    pub fn id(self) -> ProbeId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginHandle {
    pub(super) id: PluginId,
    pub(super) context: u64,
}

impl PluginHandle {
    pub fn id(self) -> PluginId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GeneratedModuleHandle {
    pub(super) id: GeneratedModuleId,
    pub(super) context: u64,
}

impl GeneratedModuleHandle {
    pub fn id(self) -> GeneratedModuleId {
        self.id
    }
}
