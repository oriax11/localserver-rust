pub trait HttpResponseCommon {
    fn peek(&self) -> &[u8];
    fn next(&mut self);
    fn is_finished(&self) -> bool;

    fn remaining(&self) -> &[u8] { self.peek() }
    fn advance(&mut self, n: usize) { self.next() }
}

pub struct SimpleResponse {
    data: Vec<u8>,
    index: usize,
}

impl SimpleResponse {
    pub fn new(data: Vec<u8>) -> Self { Self { data, index: 0 } }
}

impl HttpResponseCommon for SimpleResponse {
    fn peek(&self) -> &[u8] { &self.data[self.index..] }
    fn next(&mut self) { self.index = self.data.len() }
    fn is_finished(&self) -> bool { self.index >= self.data.len() }
    fn remaining(&self) -> &[u8] { &self.data[self.index..] }
    fn advance(&mut self, n: usize) { self.index = std::cmp::min(self.index + n, self.data.len()) }
}
