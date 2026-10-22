//! Exhaustive detector owner identities and shared-index requirements.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum DetectorOwner {
    GeneratedVsSource,
    LargeFile,
    #[cfg(feature = "graph-analysis")]
    ToolingOverlap,
    StalePathReference,
    StaleLinkReference,
    StaleSectionAnchor,
    FunctionComplexity,
    TooManyParameters,
    SectionDividerComment,
    InvalidSuppression,
    MagicLiteralCluster,
    IdentifierVerbosity,
    IdentifierTersity,
    StringlyTypedParameter,
    FunctionLocalImport,
    DuplicateImport,
    NestedQuadraticIteration,
    RepeatedSortInLoop,
    CacheMissOpportunity,
    RepeatedFormatAllocation,
    SortForExtremum,
    FullSortForTopK,
    RepeatedCollectionTraversal,
    HotPathLinearMembership,
    NPlusOneQueryInLoop,
    LoopCarriedCopyAccumulation,
    InvariantCollectionCopyInLoop,
    RepeatedRecursiveSubproblem,
    NestedCrossCollectionTraversal,
    LoopInvariantRegexCompilation,
    ImmutableStringConcatenationInLoop,
    ManualCollectionCopyLoop,
    EagerFallbackComputation,
    IntermediateCollectionMaterialization,
    RangeForEachIteration,
    LoopInvariantComputation,
    KnownSizePreallocation,
    BulkCollectionExtension,
    MapEntryOpportunity,
    SiblingCallRedundancy,
    CognitiveComplexity,
    NpathComplexity,
    HalsteadInformationDensity,
    IdentifierStutter,
    EscapeHatchTypeDensity,
    CollectionParameterMutation,
    OutParameter,
    ExcessiveNestingDepth,
    ClassStructureMetrics,
    GodModule,
    SourceDuplicateDetection,
    ArchitecturalLayering,
    PackageCouplingMetrics,
    #[cfg(feature = "graph-analysis")]
    RepoGraphIsolatedSourceFile,
    #[cfg(feature = "graph-analysis")]
    RepoGraphHighFanoutSourceFile,
    #[cfg(feature = "graph-analysis")]
    RepoGraphImportCycle,
    #[cfg(feature = "graph-analysis")]
    ShotgunSurgeryCluster,
    #[cfg(feature = "graph-analysis")]
    LargeFileSplitCandidate,
    #[cfg(feature = "graph-analysis")]
    SmallFileConsolidationCandidate,
    LoopArtifactRewrite,
    LatestOnlyUnboundedCache,
    RepeatedParseScanSerialize,
    BlockingIoInAsyncContext,
    PerItemExternalIo,
    RepeatedExpensiveInitialization,
    RequestPathRepositoryScan,
    ChurnComplexityHotspot,
    TooManyLocals,
    TooManyReturns,
    TooManyStatements,
    NamingConvention,
    TodoUnimplementedInProduction,
    AssertInProduction,
    EvalInProduction,
    HardcodedCredential,
    UnsafeDeserialization,
    BindAllInterfaces,
    TryExceptPass,
    WorldWritableFile,
    HardcodedTmpDirectory,
    FlaskDebugTrue,
    RandomForSecurity,
    RequestNoCertVerify,
    XmlXxeVulnerable,
    SslWithBadDefaults,
    SshNoHostKeyVerification,
    AssignmentInOperand,
    ResourceNotClosed,
    EmptyConditionalBranch,
    UnreachableCode,
    ConstantCondition,
    CppRawPtrOwnership,
    RubyDoubleBang,
    CsharpLinqEnumeration,
    KotlinEmptyWhen,
    ExitInProduction,
    UnhandledTerminationInProduction,
    SqlInjection,
    ShellExecution,
    InsecureCryptography,
    ConsoleOutputInProduction,
    DebuggerInProduction,
    EmptyCatchBlock,
    NoAwaitInLoop,
    RequireAtomicUpdates,
    DeadCode,
    OrphanSymbol,
    TestWithoutAssertion,
    TautologicalAssertion,
    SkippedTestWithoutReason,
    MutableDefaultArgument,
    FloatEqualityComparison,
    NetworkCallWithoutTimeout,
    RetryWithoutBackoff,
    EagerLogFormatting,
    UnboundedConcurrency,
    UnboundedChannelOrQueue,
    PassThroughWrapper,
    SingleImplementationAbstraction,
    QuadraticFrontRemoval,
    CatastrophicBacktrackingRegex,
    UnusedDeclaredDependency,
    DiscardedFallibleResult,
    GoLogFatal,
    GoErrcheck,
    UnusedAssignment,
    GoRepeatedStringLiteral,
    JavaInefficientStringBuffer,
    KtNonNullAssertion,
    KtRequireCheckMisuse,
    UnsafeTypeCast,
}

impl DetectorOwner {
    pub(crate) const fn uses_complexity_index(self) -> bool {
        matches!(
            self,
            Self::FunctionComplexity
                | Self::CognitiveComplexity
                | Self::NpathComplexity
                | Self::HalsteadInformationDensity
                | Self::ChurnComplexityHotspot
        )
    }

    pub(crate) const fn uses_optimization_index(self) -> bool {
        matches!(
            self,
            Self::CacheMissOpportunity
                | Self::RepeatedFormatAllocation
                | Self::LoopInvariantRegexCompilation
                | Self::ImmutableStringConcatenationInLoop
                | Self::ManualCollectionCopyLoop
                | Self::EagerFallbackComputation
                | Self::IntermediateCollectionMaterialization
                | Self::RangeForEachIteration
                | Self::LoopInvariantComputation
                | Self::KnownSizePreallocation
                | Self::BulkCollectionExtension
                | Self::MapEntryOpportunity
        )
    }

    pub(crate) const fn uses_algorithmic_index(self) -> bool {
        let algorithmic = matches!(
            self,
            Self::NestedQuadraticIteration
                | Self::QuadraticFrontRemoval
                | Self::RepeatedSortInLoop
                | Self::SortForExtremum
                | Self::FullSortForTopK
                | Self::RepeatedCollectionTraversal
                | Self::HotPathLinearMembership
                | Self::NPlusOneQueryInLoop
                | Self::LoopCarriedCopyAccumulation
                | Self::InvariantCollectionCopyInLoop
                | Self::RepeatedRecursiveSubproblem
                | Self::NestedCrossCollectionTraversal
        );
        algorithmic || self.uses_optimization_index()
    }

    pub(crate) const fn uses_systems_index(self) -> bool {
        let local = matches!(
            self,
            Self::ArchitecturalLayering
                | Self::PackageCouplingMetrics
                | Self::LoopArtifactRewrite
                | Self::LatestOnlyUnboundedCache
                | Self::RepeatedParseScanSerialize
                | Self::BlockingIoInAsyncContext
                | Self::PerItemExternalIo
                | Self::RepeatedExpensiveInitialization
                | Self::RequestPathRepositoryScan
        );
        local
    }

    /// Owners that follow calls through the shared call graph.
    pub(crate) const fn uses_call_graph(self) -> bool {
        self.uses_algorithmic_index()
            || self.uses_systems_index()
            || self.uses_optimization_index()
            || matches!(self, Self::RandomForSecurity)
    }

    pub(crate) const fn uses_structure_index(self) -> bool {
        self.uses_algorithmic_index()
            || self.uses_systems_index()
            || matches!(
                self,
                Self::RandomForSecurity
                    | Self::TooManyParameters
                    | Self::SiblingCallRedundancy
                    | Self::StringlyTypedParameter
                    | Self::FunctionLocalImport
                    | Self::DuplicateImport
                    | Self::CollectionParameterMutation
                    | Self::OutParameter
                    | Self::IdentifierStutter
                    | Self::ExcessiveNestingDepth
                    | Self::ClassStructureMetrics
                    | Self::GodModule
                    | Self::TooManyLocals
                    | Self::TooManyReturns
                    | Self::TooManyStatements
                    | Self::NamingConvention
                    | Self::OrphanSymbol
            )
    }
}
