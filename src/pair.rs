#[derive(PartialEq, Eq, Clone, Copy, PartialOrd)]
pub struct Pair<A, B> {
    pub a: A,
    pub b: B,
}

impl<A, B> Pair<A, B> {
    pub fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}

impl<A, B> PartialEq<(A, B)> for Pair<A, B>
where
    A: PartialEq,
    B: PartialEq,
{
    fn eq(&self, other: &(A, B)) -> bool {
        self.a == other.0 && self.b == other.1
    }
}

impl<A, B> Ord for Pair<A, B>
where
    A: Ord,
    B: Ord,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.a.cmp(&other.a)
    }
}
