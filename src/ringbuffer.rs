#[derive(Clone, Debug)]
pub struct Ringbuffer<T> {
    data: Box<[Option<T>]>,
    index: usize,
}

impl<T: Clone> Ringbuffer<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: vec![None; capacity].into_boxed_slice(),
            index: 0,
        }
    }

    pub fn push(&mut self, item: T) {
        self.data[self.index] = Some(item);
        self.index = (self.index + 1) % self.data.len();
    }
}

impl<T: PartialEq> Ringbuffer<T> {
    pub fn contains(&self, item: &T) -> bool {
        self.data.iter().any(|x| x.as_ref() == Some(item))
    }
}
