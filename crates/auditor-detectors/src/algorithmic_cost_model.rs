use auditor_ast::SourceSpan;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CostTermKind {
    Constant,
    Linear,
    Logarithmic,
    Sort,
    RepeatedCall,
    CopyAllocation,
    Exponential,
    Unknown,
}

impl CostTermKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Constant => "constant",
            Self::Linear => "linear",
            Self::Logarithmic => "logarithmic",
            Self::Sort => "sort",
            Self::RepeatedCall => "repeated_call",
            Self::CopyAllocation => "copy_allocation",
            Self::Exponential => "exponential",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CostTerm {
    kind: CostTermKind,
    symbol: String,
    span: Option<SourceSpan>,
}

impl CostTerm {
    fn new(kind: CostTermKind, symbol: impl Into<String>, span: Option<SourceSpan>) -> Self {
        Self {
            kind,
            symbol: symbol.into(),
            span,
        }
    }

    fn evidence(&self) -> String {
        let span = self
            .span
            .map(span_text)
            .unwrap_or_else(|| "aggregate".to_string());
        format!("{}:{}@{span}", self.kind.as_str(), self.symbol)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CostExpression {
    Term(CostTerm),
    Sum(Vec<CostExpression>),
    Max(Vec<CostExpression>),
    Product(Vec<CostExpression>),
}

impl CostExpression {
    pub(crate) fn term(
        kind: CostTermKind,
        symbol: impl Into<String>,
        span: Option<SourceSpan>,
    ) -> Self {
        Self::Term(CostTerm::new(kind, symbol, span))
    }

    pub(crate) fn sum(terms: Vec<Self>) -> Self {
        Self::Sum(terms)
    }

    pub(crate) fn max(branches: Vec<Self>) -> Self {
        Self::Max(branches)
    }

    pub(crate) fn product(factors: Vec<Self>) -> Self {
        Self::Product(factors)
    }

    pub(crate) fn normalized(&self) -> String {
        match self {
            Self::Term(term) => term.evidence(),
            Self::Sum(terms) => compose("sum", terms),
            Self::Max(terms) => compose("max", terms),
            Self::Product(terms) => compose("product", terms),
        }
    }

    pub(crate) fn terms(&self) -> Vec<String> {
        let mut terms = Vec::new();
        self.collect_terms(&mut terms);
        terms
    }

    pub(crate) const fn composition(&self) -> &'static str {
        match self {
            Self::Term(_) => "term",
            Self::Sum(_) => "sum",
            Self::Max(_) => "branch_max",
            Self::Product(_) => "loop_product",
        }
    }

    fn collect_terms(&self, output: &mut Vec<String>) {
        match self {
            Self::Term(term) => output.push(term.evidence()),
            Self::Sum(items) | Self::Max(items) | Self::Product(items) => {
                for item in items {
                    item.collect_terms(output);
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AlgorithmicCost {
    pub(crate) observed: CostExpression,
    pub(crate) alternative: CostExpression,
    pub(crate) confidence_boundary: &'static str,
}

fn compose(name: &str, expressions: &[CostExpression]) -> String {
    format!(
        "{name}({})",
        expressions
            .iter()
            .map(CostExpression::normalized)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn span_text(span: SourceSpan) -> String {
    format!(
        "{}:{}-{}:{}",
        span.start_line, span.start_column, span.end_line, span.end_column
    )
}
