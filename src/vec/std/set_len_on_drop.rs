// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
// Set the length of the vec when the `VVecSetLenOnDrop` value goes out of scope.
//
// The idea is: The length field in VVecSetLenOnDrop is a local variable
// that the optimizer will see does not alias with any stores through the VVec's data
// pointer. This is a workaround for alias analysis issue #32155
pub(super) struct VVecSetLenOnDrop<'a> {
    len: &'a mut usize,
    local_len: usize,
}

impl<'a> VVecSetLenOnDrop<'a> {
    #[inline]
    pub(super) fn new(len: &'a mut usize) -> Self {
        VVecSetLenOnDrop { local_len: *len, len }
    }

    #[inline]
    pub(super) fn increment_len(&mut self, increment: usize) {
        self.local_len += increment;
    }

    #[inline]
    pub(super) fn current_len(&self) -> usize {
        self.local_len
    }
}

impl Drop for VVecSetLenOnDrop<'_> {
    #[inline]
    fn drop(&mut self) {
        *self.len = self.local_len;
    }
}
