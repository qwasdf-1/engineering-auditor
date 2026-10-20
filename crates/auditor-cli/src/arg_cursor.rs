pub(crate) struct ArgCursor<'a> {
    args: &'a [String],
    index: usize,
}

impl<'a> ArgCursor<'a> {
    pub(crate) fn new(args: &'a [String]) -> Self {
        Self { args, index: 0 }
    }

    pub(crate) fn next(&mut self) -> Option<&'a str> {
        let value = self.args.get(self.index).map(String::as_str);
        self.index += usize::from(value.is_some());
        value
    }

    pub(crate) fn required(&mut self, option: &str) -> Result<&'a str, String> {
        self.next()
            .ok_or_else(|| format!("{option} requires a value"))
    }
}
