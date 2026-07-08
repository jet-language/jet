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
            id: TargetId,
            context: u64,
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
    id: TargetId,
    context: u64,
}

impl TargetRef {
    pub fn id(self) -> TargetId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionHandle {
    id: ActionId,
    context: u64,
}

impl ActionHandle {
    pub fn id(self) -> ActionId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolchainHandle {
    id: ToolchainId,
    context: u64,
}

impl ToolchainHandle {
    pub fn id(self) -> ToolchainId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SigningIdentityHandle {
    id: SigningIdentityId,
    context: u64,
}

impl SigningIdentityHandle {
    pub fn id(self) -> SigningIdentityId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProbeHandle {
    id: ProbeId,
    context: u64,
}

impl ProbeHandle {
    pub fn id(self) -> ProbeId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginHandle {
    id: PluginId,
    context: u64,
}

impl PluginHandle {
    pub fn id(self) -> PluginId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GeneratedModuleHandle {
    id: GeneratedModuleId,
    context: u64,
}

impl GeneratedModuleHandle {
    pub fn id(self) -> GeneratedModuleId {
        self.id
    }
}
