use super::*;
use registry::DetectorOwner;

pub(super) fn run_detector_owner(
    owner: DetectorOwner,
    context: &mut DetectorContext<'_>,
) -> Vec<Finding> {
    let findings = run_foundation_owner(owner, context);
    let findings = findings.or_else(|| run_structural_owner(owner, context));
    let findings = findings.or_else(|| run_systems_owner(owner, context));
    let findings = findings.or_else(|| run_correctness_owner(owner, context));
    let findings = findings.or_else(|| run_error_owner(owner, context));
    let findings = findings.or_else(|| run_language_owner(owner, context));
    findings
        .unwrap_or_else(|| unreachable!("registered detector owner {owner:?} has no dispatcher"))
}

fn run_foundation_owner(
    owner: DetectorOwner,
    context: &mut DetectorContext<'_>,
) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::GeneratedVsSource => generated_vs_source_findings(context.files),
        DetectorOwner::LargeFile => {
            bloat::large_file_findings(context.root, context.files, context.config, context.cache)
        }
        #[cfg(feature = "graph-analysis")]
        DetectorOwner::ToolingOverlap => structural::tooling_overlap_findings(context.files),
        DetectorOwner::StalePathReference => {
            markdown::stale_path_reference_findings(context.root, context.files, context.cache)
        }
        DetectorOwner::StaleLinkReference => {
            markdown::stale_link_reference_findings(context.root, context.files, context.cache)
        }
        DetectorOwner::StaleSectionAnchor => {
            markdown::stale_section_anchor_findings(context.root, context.files, context.cache)
        }
        DetectorOwner::SectionDividerComment => {
            lexical::section_divider_comment_findings(context.files, context.cache)
        }
        DetectorOwner::MagicLiteralCluster => {
            lexical::magic_literal_cluster_findings(context.files, context.cache)
        }
        DetectorOwner::IdentifierVerbosity => {
            lexical::identifier_verbosity_findings(context.files, context.cache)
        }
        DetectorOwner::IdentifierTersity => {
            lexical::identifier_tersity_findings(context.files, context.cache)
        }
        DetectorOwner::IdentifierStutter => structural_semantic::identifier_stutter_findings(
            context.structure.as_ref().expect("structure index"),
        ),
        DetectorOwner::SourceDuplicateDetection => {
            bloat::source_duplicate_detection_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        _ => return None,
    })
}

fn run_structural_owner(
    owner: DetectorOwner,
    context: &mut DetectorContext<'_>,
) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::FunctionComplexity => structural::function_complexity_findings(
            context.files,
            context.method,
            context.cache,
            context.complexity.as_ref().expect("complexity index"),
        ),
        DetectorOwner::TooManyParameters => structural_semantic::too_many_parameters_findings(
            context.structure.as_ref().expect("structure index"),
        ),
        DetectorOwner::StringlyTypedParameter => {
            structural_semantic::stringly_typed_parameter_findings(
                context.structure.as_ref().expect("structure index"),
            )
        }
        DetectorOwner::FunctionLocalImport => structural_semantic::function_local_import_findings(
            context.structure.as_ref().expect("structure index"),
        ),
        DetectorOwner::DuplicateImport => structural_semantic::duplicate_import_findings(
            context.structure.as_ref().expect("structure index"),
        ),
        DetectorOwner::SiblingCallRedundancy => {
            structural_semantic::sibling_call_redundancy_findings(
                context.structure.as_ref().expect("structure index"),
            )
        }
        DetectorOwner::CognitiveComplexity => structural::cognitive_complexity_findings(
            context.method,
            context.complexity.as_ref().expect("complexity index"),
        ),
        DetectorOwner::NpathComplexity => structural::npath_complexity_findings(
            context.method,
            context.complexity.as_ref().expect("complexity index"),
        ),
        DetectorOwner::HalsteadInformationDensity => {
            structural::halstead_information_density_findings(
                context.files,
                context.method,
                context.cache,
                context.complexity.as_ref().expect("complexity index"),
            )
        }
        DetectorOwner::EscapeHatchTypeDensity => {
            structural::escape_hatch_type_density_findings(context.files, context.cache)
        }
        DetectorOwner::CollectionParameterMutation => {
            structural_semantic::collection_parameter_mutation_findings(
                context.structure.as_ref().expect("structure index"),
            )
        }
        DetectorOwner::OutParameter => structural_semantic::out_parameter_findings(
            context.structure.as_ref().expect("structure index"),
        ),
        DetectorOwner::ExcessiveNestingDepth => {
            structural_semantic::excessive_nesting_depth_findings(
                context.structure.as_ref().expect("structure index"),
            )
        }
        DetectorOwner::ClassStructureMetrics => {
            structural_semantic::class_structure_metrics_findings(
                context.structure.as_ref().expect("structure index"),
            )
        }
        DetectorOwner::GodModule => structural_semantic::god_module_findings(
            context.structure.as_ref().expect("structure index"),
        ),
        DetectorOwner::TooManyLocals => structural_semantic::too_many_locals_findings(
            context.structure.as_ref().expect("structure index"),
        ),
        DetectorOwner::TooManyReturns => structural_semantic::too_many_returns_findings(
            context.structure.as_ref().expect("structure index"),
        ),
        DetectorOwner::TooManyStatements => structural_semantic::too_many_statements_findings(
            context.structure.as_ref().expect("structure index"),
        ),
        DetectorOwner::TestWithoutAssertion => testing_quality::test_without_assertion_findings(
            context.files,
            context.method,
            context.cache,
            &mut context.repo_ast,
        ),
        DetectorOwner::TautologicalAssertion => testing_quality::tautological_assertion_findings(
            context.files,
            context.method,
            context.cache,
            &mut context.repo_ast,
        ),
        DetectorOwner::CatastrophicBacktrackingRegex => {
            regex_backtracking::catastrophic_backtracking_regex_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::UnusedDeclaredDependency => {
            dependency_usage::unused_declared_dependency_findings(
                context.root,
                context.files,
                context.context_files,
                context.cache,
            )
        }
        DetectorOwner::DiscardedFallibleResult => {
            discarded_results::discarded_fallible_result_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::NetworkCallWithoutTimeout => {
            network_timeouts::network_call_without_timeout_findings(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            )
        }
        _ => return None,
    })
}

fn run_systems_owner(owner: DetectorOwner, context: &DetectorContext<'_>) -> Option<Vec<Finding>> {
    let findings = run_algorithmic_owner(owner, context);
    let findings = findings.or_else(|| run_optimization_owner(owner, context));
    let findings = findings.or_else(|| run_layering_owner(owner, context));
    #[cfg(feature = "graph-analysis")]
    let findings = findings.or_else(|| run_graph_owner(owner, context));
    let findings = findings.or_else(|| run_system_economics_owner(owner, context));
    findings.or_else(|| run_churn_owner(owner, context))
}

fn run_algorithmic_owner(
    owner: DetectorOwner,
    context: &DetectorContext<'_>,
) -> Option<Vec<Finding>> {
    let index = context.algorithmic.as_ref()?;
    run_established_algorithmic_owner(owner, index)
        .or_else(|| run_cost_algorithmic_owner(owner, index))
}

fn run_established_algorithmic_owner(
    owner: DetectorOwner,
    index: &algorithmic_facts::AlgorithmicIndex,
) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::NestedQuadraticIteration => {
            algorithmic::nested_quadratic_iteration_findings(index)
        }
        DetectorOwner::RepeatedSortInLoop => algorithmic::repeated_sort_in_loop_findings(index),
        DetectorOwner::QuadraticFrontRemoval => {
            algorithmic::quadratic_front_removal_findings(index)
        }
        DetectorOwner::SortForExtremum => algorithmic::sort_for_extremum_findings(index),
        DetectorOwner::FullSortForTopK => algorithmic::full_sort_for_top_k_findings(index),
        DetectorOwner::RepeatedCollectionTraversal => {
            algorithmic::repeated_collection_traversal_findings(index)
        }
        DetectorOwner::HotPathLinearMembership => {
            algorithmic::hot_path_linear_membership_findings(index)
        }
        DetectorOwner::NPlusOneQueryInLoop => algorithmic::n_plus_one_query_in_loop_findings(index),
        _ => return None,
    })
}

fn run_cost_algorithmic_owner(
    owner: DetectorOwner,
    index: &algorithmic_facts::AlgorithmicIndex,
) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::LoopCarriedCopyAccumulation => {
            algorithmic::loop_carried_copy_accumulation_findings(index)
        }
        DetectorOwner::InvariantCollectionCopyInLoop => {
            algorithmic::invariant_collection_copy_in_loop_findings(index)
        }
        DetectorOwner::RepeatedRecursiveSubproblem => {
            algorithmic::repeated_recursive_subproblem_findings(index)
        }
        DetectorOwner::NestedCrossCollectionTraversal => {
            algorithmic::nested_cross_collection_traversal_findings(index)
        }
        _ => return None,
    })
}

fn run_optimization_owner(
    owner: DetectorOwner,
    context: &DetectorContext<'_>,
) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::CacheMissOpportunity => optimization::cache_miss_opportunity_findings(
            context.optimization.as_ref().expect("optimization index"),
        ),
        DetectorOwner::RepeatedFormatAllocation => {
            optimization::repeated_format_allocation_findings(
                context.optimization.as_ref().expect("optimization index"),
            )
        }
        DetectorOwner::LoopInvariantRegexCompilation => {
            optimization::loop_invariant_regex_compilation_findings(
                context.optimization.as_ref().expect("optimization index"),
            )
        }
        DetectorOwner::ImmutableStringConcatenationInLoop => {
            optimization::immutable_string_concatenation_in_loop_findings(
                context.optimization.as_ref().expect("optimization index"),
            )
        }
        DetectorOwner::ManualCollectionCopyLoop => {
            optimization::manual_collection_copy_loop_findings(
                context.optimization.as_ref().expect("optimization index"),
            )
        }
        DetectorOwner::EagerFallbackComputation => {
            optimization::eager_fallback_computation_findings(
                context.optimization.as_ref().expect("optimization index"),
            )
        }
        DetectorOwner::IntermediateCollectionMaterialization => {
            optimization::intermediate_collection_materialization_findings(
                context.optimization.as_ref().expect("optimization index"),
            )
        }
        DetectorOwner::RangeForEachIteration => optimization::range_for_each_iteration_findings(
            context.optimization.as_ref().expect("optimization index"),
        ),
        DetectorOwner::LoopInvariantComputation => {
            optimization::loop_invariant_computation_findings(
                context.optimization.as_ref().expect("optimization index"),
            )
        }
        DetectorOwner::KnownSizePreallocation => optimization::known_size_preallocation_findings(
            context.optimization.as_ref().expect("optimization index"),
        ),
        DetectorOwner::BulkCollectionExtension => optimization::bulk_collection_extension_findings(
            context.optimization.as_ref().expect("optimization index"),
        ),
        DetectorOwner::MapEntryOpportunity => optimization::map_entry_opportunity_findings(
            context.optimization.as_ref().expect("optimization index"),
        ),
        _ => return None,
    })
}

fn run_layering_owner(owner: DetectorOwner, context: &DetectorContext<'_>) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::ArchitecturalLayering => layering::architectural_layering_findings(
            context.root,
            context.systems.as_ref().expect("systems index"),
            context.config,
        ),
        DetectorOwner::PackageCouplingMetrics => layering::package_coupling_metrics_findings(
            context.systems.as_ref().expect("systems index"),
        ),
        _ => return None,
    })
}

#[cfg(feature = "graph-analysis")]
fn run_graph_owner(owner: DetectorOwner, context: &DetectorContext<'_>) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::RepoGraphIsolatedSourceFile => {
            repository_graph::repo_graph_isolated_source_file_findings(context.graph_inputs)
        }
        DetectorOwner::RepoGraphHighFanoutSourceFile => {
            repository_graph::repo_graph_high_fanout_source_file_findings(context.graph_inputs)
        }
        DetectorOwner::RepoGraphImportCycle => {
            repository_graph::repo_graph_import_cycle_findings(context.graph_inputs)
        }
        DetectorOwner::ShotgunSurgeryCluster => {
            repository_graph::shotgun_surgery_cluster_findings(context.graph_inputs)
        }
        DetectorOwner::LargeFileSplitCandidate => {
            repository_graph::large_file_split_candidate_findings(context.graph_inputs)
        }
        DetectorOwner::SmallFileConsolidationCandidate => {
            repository_graph::small_file_consolidation_candidate_findings(context.graph_inputs)
        }
        _ => return None,
    })
}

fn run_system_economics_owner(
    owner: DetectorOwner,
    context: &DetectorContext<'_>,
) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::LoopArtifactRewrite => systems::loop_artifact_rewrite_findings(
            context.systems.as_ref().expect("systems index"),
        ),
        DetectorOwner::LatestOnlyUnboundedCache => systems::latest_only_unbounded_cache_findings(
            context.systems.as_ref().expect("systems index"),
        ),
        DetectorOwner::RepeatedParseScanSerialize => {
            systems::repeated_parse_scan_serialize_findings(
                context.systems.as_ref().expect("systems index"),
            )
        }
        DetectorOwner::BlockingIoInAsyncContext => systems::blocking_io_in_async_context_findings(
            context.systems.as_ref().expect("systems index"),
        ),
        DetectorOwner::PerItemExternalIo => {
            systems::per_item_external_io_findings(context.systems.as_ref().expect("systems index"))
        }
        DetectorOwner::RepeatedExpensiveInitialization => {
            systems::repeated_expensive_initialization_findings(
                context.systems.as_ref().expect("systems index"),
            )
        }
        DetectorOwner::RequestPathRepositoryScan => systems::request_path_repository_scan_findings(
            context.systems.as_ref().expect("systems index"),
        ),
        _ => return None,
    })
}

fn run_churn_owner(owner: DetectorOwner, context: &DetectorContext<'_>) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::ChurnComplexityHotspot => {
            let churn = churn::collect_file_churn(context.root);
            systems::churn_complexity_hotspot_findings(
                context.files,
                &churn,
                context.complexity.as_ref().expect("complexity index"),
            )
        }
        _ => return None,
    })
}

fn run_correctness_owner(
    owner: DetectorOwner,
    context: &mut DetectorContext<'_>,
) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::TodoUnimplementedInProduction => {
            correctness::todo_unimplemented_in_production_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::AssertInProduction => {
            correctness::assert_in_production_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::EvalInProduction => {
            correctness::eval_in_production_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::HardcodedCredential => security_rules::hardcoded_credential_findings(
            ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ),
            context.config,
        ),
        DetectorOwner::UnsafeDeserialization => security_rules::unsafe_deserialization_findings(
            ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ),
            context.config,
        ),
        DetectorOwner::BindAllInterfaces => security_rules::bind_all_interfaces_findings(
            ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ),
            context.config,
        ),
        DetectorOwner::TryExceptPass => {
            security_rules::try_except_pass_findings(context.files, context.cache, context.config)
        }
        DetectorOwner::WorldWritableFile => security_rules::world_writable_file_findings(
            ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ),
            context.config,
        ),
        DetectorOwner::HardcodedTmpDirectory => security_rules::hardcoded_tmp_directory_findings(
            ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ),
            context.config,
        ),
        DetectorOwner::FlaskDebugTrue => {
            security_rules::flask_debug_true_findings(context.files, context.cache, context.config)
        }
        DetectorOwner::RandomForSecurity => security_rules::random_for_security_findings(
            ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ),
            context.config,
        ),
        DetectorOwner::RequestNoCertVerify => security_rules::request_no_cert_verify_findings(
            ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ),
            context.config,
        ),
        DetectorOwner::XmlXxeVulnerable => security_rules::xml_xxe_vulnerable_findings(
            ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ),
            context.config,
        ),
        DetectorOwner::SslWithBadDefaults => security_rules::ssl_with_bad_defaults_findings(
            ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ),
            context.config,
        ),
        DetectorOwner::SshNoHostKeyVerification => {
            security_rules::ssh_no_host_key_verification_findings(
                ast_scan::AstScan::new(
                    context.files,
                    context.method,
                    context.cache,
                    &mut context.repo_ast,
                ),
                context.config,
            )
        }
        _ => return None,
    })
}

fn run_error_owner(
    owner: DetectorOwner,
    context: &mut DetectorContext<'_>,
) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::AssignmentInOperand => {
            errorprone::assignment_in_operand_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::ResourceNotClosed => {
            errorprone::resource_not_closed_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::EmptyConditionalBranch => {
            errorprone::empty_conditional_branch_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::UnreachableCode => {
            control_flow::unreachable_code_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::ConstantCondition => {
            control_flow::constant_condition_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::CppRawPtrOwnership => {
            errorprone::cpp_raw_ptr_ownership_findings(context.files, context.cache)
        }
        DetectorOwner::RubyDoubleBang => {
            errorprone::ruby_double_bang_findings(context.files, context.cache)
        }
        DetectorOwner::CsharpLinqEnumeration => {
            errorprone::csharp_linq_enumeration_findings(context.files, context.cache)
        }
        DetectorOwner::KotlinEmptyWhen => {
            errorprone::kotlin_empty_when_findings(context.files, context.cache)
        }
        DetectorOwner::ExitInProduction => {
            errorprone::exit_in_production_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::UnhandledTerminationInProduction => {
            errorprone::unhandled_termination_in_production_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::SqlInjection => security::sql_injection_findings(ast_scan::AstScan::new(
            context.files,
            context.method,
            context.cache,
            &mut context.repo_ast,
        )),
        DetectorOwner::ShellExecution => {
            security::shell_execution_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::InsecureCryptography => {
            security::insecure_cryptography_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::ConsoleOutputInProduction => {
            errorprone::console_output_in_production_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::DebuggerInProduction => {
            errorprone::debugger_in_production_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::EmptyCatchBlock => {
            errorprone::empty_catch_block_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::NoAwaitInLoop => {
            errorprone::no_await_in_loop_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::RequireAtomicUpdates => {
            errorprone::require_atomic_updates_findings(context.files, context.cache)
        }
        _ => return None,
    })
}

fn run_language_owner(
    owner: DetectorOwner,
    context: &mut DetectorContext<'_>,
) -> Option<Vec<Finding>> {
    Some(match owner {
        DetectorOwner::DeadCode => dead_code::dead_code_findings_with_context(
            context.files,
            context.context_files,
            context.method,
            context.cache,
            &mut context.repo_ast,
        ),
        DetectorOwner::OrphanSymbol => structural_semantic::orphan_symbol_findings(
            context.structure.as_ref().expect("structure index"),
        ),
        DetectorOwner::GoLogFatal => golang::go_log_fatal_findings(
            &files_for_language(context.files, Language::Go),
            context.cache,
        ),
        DetectorOwner::GoErrcheck => golang::go_errcheck_findings(
            &files_for_language(context.files, Language::Go),
            context.cache,
        ),
        DetectorOwner::UnusedAssignment => {
            golang::unused_assignment_findings(ast_scan::AstScan::new(
                context.files,
                context.method,
                context.cache,
                &mut context.repo_ast,
            ))
        }
        DetectorOwner::GoRepeatedStringLiteral => golang::go_repeated_string_literal_findings(
            context.root,
            &files_for_language(context.files, Language::Go),
            context.cache,
        ),
        DetectorOwner::JavaInefficientStringBuffer => {
            java::java_inefficient_string_buffer_findings(
                &files_for_language(context.files, Language::Java),
                context.cache,
            )
        }
        DetectorOwner::KtNonNullAssertion => kotlin::kt_non_null_assertion_findings(
            &files_for_language(context.files, Language::Kotlin),
            context.cache,
        ),
        DetectorOwner::KtRequireCheckMisuse => kotlin::kt_require_check_misuse_findings(
            &files_for_language(context.files, Language::Kotlin),
            context.cache,
        ),
        DetectorOwner::UnsafeTypeCast => kotlin::unsafe_type_cast_findings(
            &files_for_language(context.files, Language::Kotlin),
            context.cache,
        ),
        DetectorOwner::NamingConvention => {
            let files = context.files.iter().collect::<Vec<_>>();
            naming::naming_convention_findings(
                &files,
                context.cache,
                context.config,
                context.structure.as_ref().expect("structure index"),
            )
        }
        _ => return None,
    })
}

fn files_for_language(files: &[AuditedFile], language: Language) -> Vec<&AuditedFile> {
    files
        .iter()
        .filter(|file| file.language == language)
        .collect()
}
