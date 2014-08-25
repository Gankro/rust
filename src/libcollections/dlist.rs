// Copyright 2012-2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! A doubly-linked list with owned nodes.
//!
//! The `DList` allows pushing and popping elements at either end.
//!
//! `DList` implements the trait `Deque`. It should be imported with
//! `use collections::Deque`.

// DList is constructed like a singly-linked list over the field `next`.
// including the last link being None; each Node owns its `next` field.
//
// Backlinks over DList::prev are raw pointers that form a full chain in
// the reverse direction. Backlinks are inherently unsafe, as it is trivial to create
// dangling Backlinks. As such, `resolving` a backlink is considered unsafe,
// and all methods maintain safety by assuming that the DList is valid before they are
// called, and ensuring that it is valid after they are called. It is also trivial to
// upgrade mutability using Backlinks.

use core::prelude::*;

use alloc::boxed::Box;
use core::default::Default;
use core::fmt;
use core::iter;
use core::mem;
use core::ptr;
use std::hash::{Writer, Hash};

use {Mutable, Deque, MutableSeq};

/// A doubly-linked list.
pub struct DList<T> {
    length: uint,
    list_head: Link<T>,
    list_tail: Rawlink<T>,
}

type Link<T> = Option<Box<Node<T>>>;
struct Rawlink<T> { p: *mut Node<T> }

struct Node<T> {
    next: Link<T>,
    prev: Rawlink<T>,
    value: T,
}

/// An iterator over references to the items of a `DList`.
pub struct Items<'a, T> {
    head: Rawlink<T>,
    tail: Rawlink<T>,
    nelem: uint,
}

// FIXME #11820: the &'a Option<> of the Link stops clone working.
impl<'a, T> Clone for Items<'a, T> {
    fn clone(&self) -> Items<'a, T> { *self }
}

/// An iterator over mutable references to the items of a `DList`.
pub struct MutItems<'a, T> {
    list: &'a mut DList<T>,
    head: Rawlink<T>,
    tail: Rawlink<T>,
    nelem: uint,
}

/// A consuming iterator over the items of a `DList`.
#[deriving(Clone)]
pub struct MoveItems<T> {
    list: DList<T>
}

/// Rawlink is a type like Option<T> but for holding a raw pointer
impl<T> Rawlink<T> {
    /// Like Option::None for Rawlink
    fn none() -> Rawlink<T> {
        Rawlink{p: ptr::mut_null()}
    }

    /// Like Option::Some for Rawlink
    fn some(n: &Node<T>) -> Rawlink<T> {
        unsafe { Rawlink{p: mem::transmute(n) } }
    }

    /// Convert the `Rawlink` into an Option value
    unsafe fn resolve_immut<'a>(&self) -> Option<&'a Node<T>> {
        mem::transmute(self.p.to_option())
    }

    /// Convert the `Rawlink` into an Option value
    unsafe fn resolve<'a>(&mut self) -> Option<&'a mut Node<T>> {
        if self.p.is_null() {
            None
        } else {
            Some(mem::transmute(self.p))
        }
    }

    /// Return the `Rawlink` and replace with `Rawlink::none()`
    fn take(&mut self) -> Rawlink<T> {
        mem::replace(self, Rawlink::none())
    }
}

impl<T> Clone for Rawlink<T> {
    #[inline]
    fn clone(&self) -> Rawlink<T> {
        Rawlink{p: self.p}
    }
}

impl<T> Node<T> {
    fn new(v: T) -> Node<T> {
        Node{value: v, next: None, prev: Rawlink::none()}
    }
}

fn as_raw <T> (link: &Link<T>) -> Rawlink<T> {
    match *link {
        Some(ref h) => Rawlink::some(&**h),
        None => Rawlink::none(),
    }
}

/// Set the .prev field on `next`, then return `Some(next)`
fn link_with_prev<T>(mut next: Box<Node<T>>, prev: Rawlink<T>)
                  -> Link<T> {
    next.prev = prev;
    Some(next)
}

impl<T> Collection for DList<T> {
    /// Returns `true` if the `DList` is empty.
    ///
    /// This operation should compute in O(1) time.
    #[inline]
    fn is_empty(&self) -> bool {
        self.list_head.is_none()
    }

    /// Returns the length of the `DList`.
    ///
    /// This operation should compute in O(1) time.
    #[inline]
    fn len(&self) -> uint {
        self.length
    }
}

impl<T> Mutable for DList<T> {
    /// Removes all elements from the `DList`.
    ///
    /// This operation should compute in O(n) time.
    #[inline]
    fn clear(&mut self) {
        *self = DList::new()
    }
}

// private methods
impl<T> DList<T> {
    /// Add a Node first in the list
    #[inline]
    fn push_front_node(&mut self, mut new_head: Box<Node<T>>) {
        match self.list_head {
            None => {
                self.list_tail = Rawlink::some(&mut *new_head);
                self.list_head = link_with_prev(new_head, Rawlink::none());
            }
            Some(ref mut head) => {
                new_head.prev = Rawlink::none();
                head.prev = Rawlink::some(&mut *new_head);
                mem::swap(head, &mut new_head);
                head.next = Some(new_head);
            }
        }
        self.length += 1;
    }

    /// Remove the first Node and return it, or None if the list is empty
    #[inline]
    fn pop_front_node(&mut self) -> Option<Box<Node<T>>> {
        self.list_head.take().map(|mut front_node| {
            self.length -= 1;
            match front_node.next.take() {
                Some(node) => self.list_head = link_with_prev(node, Rawlink::none()),
                None => self.list_tail = Rawlink::none()
            }
            front_node
        })
    }

    /// Add a Node last in the list
    #[inline]
    fn push_back_node(&mut self, mut new_tail: Box<Node<T>>) {
        unsafe {
            match self.list_tail.resolve() {
                None => return self.push_front_node(new_tail),
                Some(tail) => {
                    self.list_tail = Rawlink::some(&mut *new_tail);
                    tail.next = link_with_prev(new_tail, Rawlink::some(tail));
                }
            }
            self.length += 1;
        }
    }

    /// Remove the last Node and return it, or None if the list is empty
    #[inline]
    fn pop_back_node(&mut self) -> Option<Box<Node<T>>> {
        unsafe {
            self.list_tail.resolve().map_or(None, |tail| {
                self.length -= 1;
                self.list_tail = tail.prev;
                match tail.prev.resolve() {
                    None => self.list_head.take(),
                    Some(tail_prev) => tail_prev.next.take()
                }
            })
        }
    }

    /// Insert a Node in the list before the given Rawlink
    ///
    /// If the given Rawlink is null, append to the end of the list, so that this
    /// function can potentially insert anywhere in the list
    ///
    /// #Safety
    /// This is safe as long as `link` is a valid node in the DList, or `null`
    #[inline]
    unsafe fn insert_before_node(&mut self, mut link: Rawlink<T>, ins_node: Box<Node<T>>) {
        match link.resolve() {
            None => self.push_back_node(ins_node),
            Some(node) => self.insert_after_node(node.prev, ins_node)
        }
    }

    /// Insert a Node in the list after the given Rawlink
    ///
    /// If the given Rawlink is null, append to the front of the list, so that this
    /// function can potentially insert anywhere in the list
    ///
    /// #Safety
    /// This is safe as long as `link` is a valid node in the DList, or `null`
    #[inline]
    unsafe fn insert_after_node(&mut self, mut link: Rawlink<T>, mut ins_node: Box<Node<T>>) {
        match link.resolve() {
            None => self.push_front_node(ins_node),
            Some(node) => {
                let next_node = match node.next.take() {
                    None => return self.push_back_node(ins_node),
                    Some(next) => next,
                };
                ins_node.next = link_with_prev(next_node, Rawlink::some(&mut *ins_node));
                node.next = link_with_prev(ins_node, Rawlink::some(node));
                self.length += 1;
            }
        }
    }

    /// Get the head of the list as a Rawlink
    #[inline]
    fn head_raw(&self) -> Rawlink<T>{
        as_raw(&self.list_head)
    }
}

impl<T> Deque<T> for DList<T> {
    /// Provides a reference to the front element, or `None` if the list is
    /// empty.
    #[inline]
    fn front<'a>(&'a self) -> Option<&'a T> {
        self.list_head.as_ref().map(|head| &head.value)
    }

    /// Provides a mutable reference to the front element, or `None` if the list
    /// is empty.
    #[inline]
    fn front_mut<'a>(&'a mut self) -> Option<&'a mut T> {
        self.list_head.as_mut().map(|head| &mut head.value)
    }

    /// Provides a reference to the back element, or `None` if the list is
    /// empty.
    #[inline]
    fn back<'a>(&'a self) -> Option<&'a T> {
        unsafe{
            self.list_tail.resolve_immut().as_ref().map(|tail| &tail.value)
        }
    }

    /// Provides a mutable reference to the back element, or `None` if the list
    /// is empty.
    #[inline]
    fn back_mut<'a>(&'a mut self) -> Option<&'a mut T> {
        unsafe {
            self.list_tail.resolve().map(|tail| &mut tail.value)
        }
    }

    /// Adds an element first in the list.
    ///
    /// This operation should compute in O(1) time.
    fn push_front(&mut self, elt: T) {
        self.push_front_node(box Node::new(elt))
    }

    /// Removes the first element and returns it, or `None` if the list is
    /// empty.
    ///
    /// This operation should compute in O(1) time.
    fn pop_front(&mut self) -> Option<T> {
        self.pop_front_node().map(|box Node{value, ..}| value)
    }
}

impl<T> MutableSeq<T> for DList<T> {
    fn push(&mut self, elt: T) {
        self.push_back_node(box Node::new(elt))
    }
    fn pop(&mut self) -> Option<T> {
        self.pop_back_node().map(|box Node{value, ..}| value)
    }
}

impl<T> Default for DList<T> {
    #[inline]
    fn default() -> DList<T> { DList::new() }
}

impl<T> DList<T> {
    /// Creates an empty `DList`.
    #[inline]
    pub fn new() -> DList<T> {
        DList{list_head: None, list_tail: Rawlink::none(), length: 0}
    }

    /// Moves the last element to the front of the list.
    ///
    /// If the list is empty, does nothing.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::collections::DList;
    ///
    /// let mut dl = DList::new();
    /// dl.push(1i);
    /// dl.push(2);
    /// dl.push(3);
    ///
    /// dl.rotate_forward();
    ///
    /// for e in dl.iter() {
    ///     println!("{}", e); // prints 3, then 1, then 2
    /// }
    /// ```
    #[inline]
    pub fn rotate_forward(&mut self) {
        self.pop_back_node().map(|tail| {
            self.push_front_node(tail)
        });
    }

    /// Moves the first element to the back of the list.
    ///
    /// If the list is empty, does nothing.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::collections::DList;
    ///
    /// let mut dl = DList::new();
    /// dl.push(1i);
    /// dl.push(2);
    /// dl.push(3);
    ///
    /// dl.rotate_backward();
    ///
    /// for e in dl.iter() {
    ///     println!("{}", e); // prints 2, then 3, then 1
    /// }
    /// ```
    #[inline]
    pub fn rotate_backward(&mut self) {
        self.pop_front_node().map(|head| {
            self.push_back_node(head)
        });
    }

    /// Adds all elements from `other` to the end of the list.
    ///
    /// This operation should compute in O(1) time.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::collections::DList;
    ///
    /// let mut a = DList::new();
    /// let mut b = DList::new();
    /// a.push(1i);
    /// a.push(2);
    /// b.push(3i);
    /// b.push(4);
    ///
    /// a.append(b);
    ///
    /// for e in a.iter() {
    ///     println!("{}", e); // prints 1, then 2, then 3, then 4
    /// }
    /// ```
    pub fn append(&mut self, mut other: DList<T>) {
        unsafe {
            match self.list_tail.resolve() {
                None => *self = other,
                Some(tail) => {
                    // Carefully empty `other`.
                    let o_tail = other.list_tail.take();
                    let o_length = other.length;
                    match other.list_head.take() {
                        None => return,
                        Some(node) => {
                            tail.next = link_with_prev(node, self.list_tail);
                            self.list_tail = o_tail;
                            self.length += o_length;
                        }
                    }
                }
            }
        }
    }

    /// Adds all elements from `other` to the beginning of the list.
    ///
    /// This operation should compute in O(1) time.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::collections::DList;
    ///
    /// let mut a = DList::new();
    /// let mut b = DList::new();
    /// a.push(1i);
    /// a.push(2);
    /// b.push(3i);
    /// b.push(4);
    ///
    /// a.prepend(b);
    ///
    /// for e in a.iter() {
    ///     println!("{}", e); // prints 3, then 4, then 1, then 2
    /// }
    /// ```
    #[inline]
    pub fn prepend(&mut self, mut other: DList<T>) {
        mem::swap(self, &mut other);
        self.append(other);
    }

    /// Inserts `elt` before the first `x` in the list where `f(x, elt)` is
    /// true, or at the end.
    ///
    /// This operation should compute in O(N) time.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::collections::DList;
    ///
    /// let mut a: DList<int> = DList::new();
    /// a.push(2i);
    /// a.push(4);
    /// a.push(7);
    /// a.push(8);
    ///
    /// // insert 11 before the first odd number in the list
    /// a.insert_when(11, |&e, _| e % 2 == 1);
    ///
    /// for e in a.iter() {
    ///     println!("{}", e); // prints 2, then 4, then 11, then 7, then 8
    /// }
    /// ```
    pub fn insert_when(&mut self, elt: T, f: |&T, &T| -> bool) {
        {
            let mut it = self.mut_iter();
            loop {
                match it.peek_next() {
                    None => break,
                    Some(x) => if f(x, &elt) { break }
                }
                it.next();
            }
            it.insert_next(elt);
        }
    }

    /// Merges `other` into this `DList`, using the function `f`.
    ///
    /// Iterates both `DList`s with `a` from self and `b` from `other`, and
    /// put `a` in the result if `f(a, b)` is true, and otherwise `b`.
    ///
    /// This operation should compute in O(max(N, M)) time.
    pub fn merge(&mut self, mut other: DList<T>, f: |&T, &T| -> bool) {
        {
            let mut it = self.mut_iter();
            loop {
                let take_a = match (it.peek_next(), other.front()) {
                    (_   , None) => return,
                    (None, _   ) => break,
                    (Some(ref mut x), Some(y)) => f(*x, y),
                };
                if take_a {
                    it.next();
                } else {
                    it.insert_next_node(other.pop_front_node().unwrap());
                }
            }
        }
        self.append(other);
    }

    /// Provides a forward iterator.
    #[inline]
    pub fn iter<'a>(&'a self) -> Items<'a, T> {
        let head_raw = self.head_raw();
        Items{
            nelem: self.len(),
            head: head_raw,
            tail: self.list_tail,
        }
    }

    /// Provides a forward iterator with mutable references.
    #[inline]
    pub fn mut_iter<'a>(&'a mut self) -> MutItems<'a, T> {
        let head_raw = self.head_raw();
        MutItems{
            nelem: self.len(),
            head: head_raw,
            tail: self.list_tail,
            list: self
        }
    }


    /// Consumes the list into an iterator yielding elements by value.
    #[inline]
    pub fn move_iter(self) -> MoveItems<T> {
        MoveItems{list: self}
    }
}

impl<T: Ord> DList<T> {
    /// Inserts `elt` sorted in ascending order.
    ///
    /// This operation should compute in O(N) time.
    #[inline]
    pub fn insert_ordered(&mut self, elt: T) {
        self.insert_when(elt, |a, b| a >= b)
    }
}

#[unsafe_destructor]
impl<T> Drop for DList<T> {
    fn drop(&mut self) {
        // Dissolve the dlist in backwards direction
        // Just dropping the list_head can lead to stack exhaustion
        // when length is >> 1_000_000
        let mut tail = self.list_tail;
        loop {
            unsafe {
                match tail.resolve() {
                    None => break,
                    Some(prev) => {
                        prev.next.take(); // release Box<Node<T>>
                        tail = prev.prev;
                    }
                }
            }
        }
        self.length = 0;
        self.list_head = None;
        self.list_tail = Rawlink::none();
    }
}


impl<'a, A> Iterator<&'a A> for Items<'a, A> {
    #[inline]
    fn next(&mut self) -> Option<&'a A> {
        if self.nelem == 0 {
            return None;
        }
        unsafe {
            self.head.resolve_immut().map(|head| {
                self.nelem -= 1;
                self.head = as_raw(&head.next);
                &head.value
            })
        }
    }

    #[inline]
    fn size_hint(&self) -> (uint, Option<uint>) {
        (self.nelem, Some(self.nelem))
    }
}

impl<'a, A> DoubleEndedIterator<&'a A> for Items<'a, A> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a A> {
        if self.nelem == 0 {
            return None;
        }
        unsafe {
            self.tail.resolve_immut().map(|tail| {
                self.nelem -= 1;
                self.tail = tail.prev;
                &tail.value
            })
        }
    }
}

impl<'a, A> ExactSize<&'a A> for Items<'a, A> {}

impl<'a, A> Items<'a, A> {
    #[inline]
    pub fn prev(&mut self) -> Option<&'a A> {
        unsafe {
            self.head.resolve_immut().and_then(|head| {
                let prev_raw = head.prev;
                prev_raw.resolve_immut().map(|prev| {
                    self.head = prev_raw;
                    self.nelem += 1;
                    &prev.value
                })
            })
        }
    }

    #[inline]
    pub fn prev_back(&mut self) -> Option<&'a A> {
        unsafe {
            self.tail.resolve_immut().and_then(|tail| {
                let prev_raw = as_raw(&tail.next);
                prev_raw.resolve_immut().map(|prev| {
                    self.tail = prev_raw;
                    self.nelem += 1;
                    &prev.value
                })
            })
        }
    }
}


// private methods
impl <'a, A> MutItems<'a, A> {
    #[inline]
    fn insert_next_node(&mut self, ins_node: Box<Node<A>>) {
        unsafe {
            self.list.insert_before_node(self.head, ins_node);
        }
    }
}

impl<'a, A> Iterator<&'a mut A> for MutItems<'a, A> {
    #[inline]
    fn next(&mut self) -> Option<&'a mut A> {
        if self.nelem == 0 {
            return None;
        }
        unsafe {
            self.head.resolve().map(|next| {
                self.nelem -= 1;
                self.head = match next.next {
                    Some(ref mut node) => Rawlink::some(&mut **node),
                    None => Rawlink::none(),
                };
                &mut next.value
            })
        }
    }

    #[inline]
    fn size_hint(&self) -> (uint, Option<uint>) {
        (self.nelem, Some(self.nelem))
    }
}

impl<'a, A> DoubleEndedIterator<&'a mut A> for MutItems<'a, A> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a mut A> {
        if self.nelem == 0 {
            return None;
        }
        unsafe {
            self.tail.resolve().map(|prev| {
                self.nelem -= 1;
                self.tail = prev.prev;
                &mut prev.value
            })
        }
    }
}

impl<'a, A> ExactSize<&'a mut A> for MutItems<'a, A> {}

/// Allows mutating a `DList` while iterating.
pub trait ListInsertion<A> {
    /// Inserts `elt` just after to the element most recently returned by
    /// `.next()`
    ///
    /// The inserted element does not appear in the iteration.
    fn insert_next(&mut self, elt: A);

    /// Provides a reference to the next element, without changing the iterator
    fn peek_next<'a>(&'a mut self) -> Option<&'a mut A>;
}

impl<'a, A> ListInsertion<A> for MutItems<'a, A> {
    #[inline]
    fn insert_next(&mut self, elt: A) {
        self.insert_next_node(box Node::new(elt))
    }

    #[inline]
    fn peek_next<'a>(&'a mut self) -> Option<&'a mut A> {
        if self.nelem == 0 {
            return None
        }
        unsafe {
            self.head.resolve().map(|head| &mut head.value)
        }
    }
}

impl<A> Iterator<A> for MoveItems<A> {
    #[inline]
    fn next(&mut self) -> Option<A> { self.list.pop_front() }

    #[inline]
    fn size_hint(&self) -> (uint, Option<uint>) {
        (self.list.length, Some(self.list.length))
    }
}

impl<A> DoubleEndedIterator<A> for MoveItems<A> {
    #[inline]
    fn next_back(&mut self) -> Option<A> { self.list.pop() }
}

impl<A> FromIterator<A> for DList<A> {
    fn from_iter<T: Iterator<A>>(iterator: T) -> DList<A> {
        let mut ret = DList::new();
        ret.extend(iterator);
        ret
    }
}

impl<A> Extendable<A> for DList<A> {
    fn extend<T: Iterator<A>>(&mut self, mut iterator: T) {
        for elt in iterator { self.push(elt); }
    }
}

impl<A: PartialEq> PartialEq for DList<A> {
    fn eq(&self, other: &DList<A>) -> bool {
        self.len() == other.len() &&
            iter::order::eq(self.iter(), other.iter())
    }

    fn ne(&self, other: &DList<A>) -> bool {
        self.len() != other.len() ||
            iter::order::ne(self.iter(), other.iter())
    }
}

impl<A: Eq> Eq for DList<A> {}

impl<A: PartialOrd> PartialOrd for DList<A> {
    fn partial_cmp(&self, other: &DList<A>) -> Option<Ordering> {
        iter::order::partial_cmp(self.iter(), other.iter())
    }
}

impl<A: Ord> Ord for DList<A> {
    #[inline]
    fn cmp(&self, other: &DList<A>) -> Ordering {
        iter::order::cmp(self.iter(), other.iter())
    }
}

impl<A: Clone> Clone for DList<A> {
    fn clone(&self) -> DList<A> {
        self.iter().map(|x| x.clone()).collect()
    }
}

impl<A: fmt::Show> fmt::Show for DList<A> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        try!(write!(f, "["));

        for (i, e) in self.iter().enumerate() {
            if i != 0 { try!(write!(f, ", ")); }
            try!(write!(f, "{}", *e));
        }

        write!(f, "]")
    }
}

impl<S: Writer, A: Hash<S>> Hash<S> for DList<A> {
    fn hash(&self, state: &mut S) {
        self.len().hash(state);
        for elt in self.iter() {
            elt.hash(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::*;
    use std::rand;
    use std::hash;
    use test::Bencher;
    use test;

    use {Deque, MutableSeq};
    use super::{DList, Node, ListInsertion};
    use vec::Vec;

    pub fn check_links<T>(list: &DList<T>) {
        let mut len = 0u;
        let mut last_ptr: Option<&Node<T>> = None;
        let mut node_ptr: &Node<T>;
        match list.list_head {
            None => { assert_eq!(0u, list.length); return }
            Some(ref node) => node_ptr = &**node,
        }
        loop {
            unsafe {
                match (last_ptr, node_ptr.prev.resolve_immut()) {
                    (None   , None      ) => {}
                    (None   , _         ) => fail!("prev link for list_head"),
                    (Some(p), Some(pptr)) => {
                        assert_eq!(p as *const Node<T>, pptr as *const Node<T>);
                    }
                    _ => fail!("prev link is none, not good"),
                }
            }
            match node_ptr.next {
                Some(ref next) => {
                    last_ptr = Some(node_ptr);
                    node_ptr = &**next;
                    len += 1;
                }
                None => {
                    len += 1;
                    break;
                }
            }
        }
        assert_eq!(len, list.length);
    }

    #[test]
    fn test_basic() {
        let mut m: DList<Box<int>> = DList::new();
        assert_eq!(m.pop_front(), None);
        assert_eq!(m.pop_back(), None);
        assert_eq!(m.pop_front(), None);
        m.push_front(box 1);
        assert_eq!(m.pop_front(), Some(box 1));
        m.push_back(box 2);
        m.push_back(box 3);
        assert_eq!(m.len(), 2);
        assert_eq!(m.pop_front(), Some(box 2));
        assert_eq!(m.pop_front(), Some(box 3));
        assert_eq!(m.len(), 0);
        assert_eq!(m.pop_front(), None);
        m.push_back(box 1);
        m.push_back(box 3);
        m.push_back(box 5);
        m.push_back(box 7);
        assert_eq!(m.pop_front(), Some(box 1));

        let mut n = DList::new();
        n.push_front(2i);
        n.push_front(3);
        {
            assert_eq!(n.front().unwrap(), &3);
            let x = n.front_mut().unwrap();
            assert_eq!(*x, 3);
            *x = 0;
        }
        {
            assert_eq!(n.back().unwrap(), &2);
            let y = n.back_mut().unwrap();
            assert_eq!(*y, 2);
            *y = 1;
        }
        assert_eq!(n.pop_front(), Some(0));
        assert_eq!(n.pop_front(), Some(1));
    }

    #[cfg(test)]
    fn generate_test() -> DList<int> {
        list_from(&[0i,1,2,3,4,5,6])
    }

    #[cfg(test)]
    fn list_from<T: Clone>(v: &[T]) -> DList<T> {
        v.iter().map(|x| (*x).clone()).collect()
    }

    #[test]
    fn test_append() {
        {
            let mut m = DList::new();
            let mut n = DList::new();
            n.push_back(2i);
            m.append(n);
            assert_eq!(m.len(), 1);
            assert_eq!(m.pop_back(), Some(2));
            check_links(&m);
        }
        {
            let mut m = DList::new();
            let n = DList::new();
            m.push_back(2i);
            m.append(n);
            assert_eq!(m.len(), 1);
            assert_eq!(m.pop_back(), Some(2));
            check_links(&m);
        }

        let v = vec![1i,2,3,4,5];
        let u = vec![9i,8,1,2,3,4,5];
        let mut m = list_from(v.as_slice());
        m.append(list_from(u.as_slice()));
        check_links(&m);
        let sum = v.append(u.as_slice());
        assert_eq!(sum.len(), m.len());
        for elt in sum.move_iter() {
            assert_eq!(m.pop_front(), Some(elt))
        }
    }

    #[test]
    fn test_prepend() {
        {
            let mut m = DList::new();
            let mut n = DList::new();
            n.push_back(2i);
            m.prepend(n);
            assert_eq!(m.len(), 1);
            assert_eq!(m.pop_back(), Some(2));
            check_links(&m);
        }

        let v = vec![1i,2,3,4,5];
        let u = vec![9i,8,1,2,3,4,5];
        let mut m = list_from(v.as_slice());
        m.prepend(list_from(u.as_slice()));
        check_links(&m);
        let sum = u.append(v.as_slice());
        assert_eq!(sum.len(), m.len());
        for elt in sum.move_iter() {
            assert_eq!(m.pop_front(), Some(elt))
        }
    }

    #[test]
    fn test_rotate() {
        let mut n: DList<int> = DList::new();
        n.rotate_backward(); check_links(&n);
        assert_eq!(n.len(), 0);
        n.rotate_forward(); check_links(&n);
        assert_eq!(n.len(), 0);

        let v = vec![1i,2,3,4,5];
        let mut m = list_from(v.as_slice());
        m.rotate_backward(); check_links(&m);
        m.rotate_forward(); check_links(&m);
        assert_eq!(v.iter().collect::<Vec<&int>>(), m.iter().collect());
        m.rotate_forward(); check_links(&m);
        m.rotate_forward(); check_links(&m);
        m.pop_front(); check_links(&m);
        m.rotate_forward(); check_links(&m);
        m.rotate_backward(); check_links(&m);
        m.push_front(9); check_links(&m);
        m.rotate_forward(); check_links(&m);
        assert_eq!(vec![3i,9,5,1,2], m.move_iter().collect());
    }

    #[test]
    fn test_iterator() {
        let m = generate_test();
        for (i, elt) in m.iter().enumerate() {
            assert_eq!(i as int, *elt);
        }
        let mut n = DList::new();
        assert_eq!(n.iter().next(), None);
        n.push_front(4i);
        let mut it = n.iter();
        assert_eq!(it.size_hint(), (1, Some(1)));
        assert_eq!(it.next().unwrap(), &4);
        assert_eq!(it.size_hint(), (0, Some(0)));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn test_iterator_clone() {
        let mut n = DList::new();
        n.push_back(2i);
        n.push_back(3);
        n.push_back(4);
        let mut it = n.iter();
        it.next();
        let mut jt = it.clone();
        assert_eq!(it.next(), jt.next());
        assert_eq!(it.next_back(), jt.next_back());
        assert_eq!(it.next(), jt.next());
    }

    #[test]
    fn test_iterator_double_end() {
        let mut n = DList::new();
        assert_eq!(n.iter().next(), None);
        n.push_front(4i);
        n.push_front(5);
        n.push_front(6);
        let mut it = n.iter();
        assert_eq!(it.size_hint(), (3, Some(3)));
        assert_eq!(it.next().unwrap(), &6);
        assert_eq!(it.size_hint(), (2, Some(2)));
        assert_eq!(it.next_back().unwrap(), &4);
        assert_eq!(it.size_hint(), (1, Some(1)));
        assert_eq!(it.next_back().unwrap(), &5);
        assert_eq!(it.next_back(), None);
        assert_eq!(it.next(), None);
    }

    #[test]
    fn test_rev_iter() {
        let m = generate_test();
        for (i, elt) in m.iter().rev().enumerate() {
            assert_eq!((6 - i) as int, *elt);
        }
        let mut n = DList::new();
        assert_eq!(n.iter().rev().next(), None);
        n.push_front(4i);
        let mut it = n.iter().rev();
        assert_eq!(it.size_hint(), (1, Some(1)));
        assert_eq!(it.next().unwrap(), &4);
        assert_eq!(it.size_hint(), (0, Some(0)));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn test_mut_iter() {
        let mut m = generate_test();
        let mut len = m.len();
        for (i, elt) in m.mut_iter().enumerate() {
            assert_eq!(i as int, *elt);
            len -= 1;
        }
        assert_eq!(len, 0);
        let mut n = DList::new();
        assert!(n.mut_iter().next().is_none());
        n.push_front(4i);
        n.push_back(5);
        let mut it = n.mut_iter();
        assert_eq!(it.size_hint(), (2, Some(2)));
        assert!(it.next().is_some());
        assert!(it.next().is_some());
        assert_eq!(it.size_hint(), (0, Some(0)));
        assert!(it.next().is_none());
    }

    #[test]
    fn test_iterator_mut_double_end() {
        let mut n = DList::new();
        assert!(n.mut_iter().next_back().is_none());
        n.push_front(4i);
        n.push_front(5);
        n.push_front(6);
        let mut it = n.mut_iter();
        assert_eq!(it.size_hint(), (3, Some(3)));
        assert_eq!(*it.next().unwrap(), 6);
        assert_eq!(it.size_hint(), (2, Some(2)));
        assert_eq!(*it.next_back().unwrap(), 4);
        assert_eq!(it.size_hint(), (1, Some(1)));
        assert_eq!(*it.next_back().unwrap(), 5);
        assert!(it.next_back().is_none());
        assert!(it.next().is_none());
    }

    #[test]
    fn test_insert_prev() {
        let mut m = list_from(&[0i,2,4,6,8]);
        let len = m.len();
        {
            let mut it = m.mut_iter();
            it.insert_next(-2);
            loop {
                match it.next() {
                    None => break,
                    Some(elt) => {
                        it.insert_next(*elt + 1);
                        match it.peek_next() {
                            Some(x) => assert_eq!(*x, *elt + 2),
                            None => assert_eq!(8, *elt),
                        }
                    }
                }
            }
            it.insert_next(0);
            it.insert_next(1);
        }
        check_links(&m);
        assert_eq!(m.len(), 3 + len * 2);
        assert_eq!(m.move_iter().collect::<Vec<int>>(), vec![-2,0,1,2,3,4,5,6,7,8,9,0,1]);
    }

    #[test]
    fn test_merge() {
        let mut m = list_from([0i, 1, 3, 5, 6, 7, 2]);
        let n = list_from([-1i, 0, 0, 7, 7, 9]);
        let len = m.len() + n.len();
        m.merge(n, |a, b| a <= b);
        assert_eq!(m.len(), len);
        check_links(&m);
        let res = m.move_iter().collect::<Vec<int>>();
        assert_eq!(res, vec![-1, 0, 0, 0, 1, 3, 5, 6, 7, 2, 7, 7, 9]);
    }

    #[test]
    fn test_insert_ordered() {
        let mut n = DList::new();
        n.insert_ordered(1i);
        assert_eq!(n.len(), 1);
        assert_eq!(n.pop_front(), Some(1));

        let mut m = DList::new();
        m.push_back(2i);
        m.push_back(4);
        m.insert_ordered(3);
        check_links(&m);
        assert_eq!(vec![2,3,4], m.move_iter().collect::<Vec<int>>());
    }

    #[test]
    fn test_mut_rev_iter() {
        let mut m = generate_test();
        for (i, elt) in m.mut_iter().rev().enumerate() {
            assert_eq!((6-i) as int, *elt);
        }
        let mut n = DList::new();
        assert!(n.mut_iter().rev().next().is_none());
        n.push_front(4i);
        let mut it = n.mut_iter().rev();
        assert!(it.next().is_some());
        assert!(it.next().is_none());
    }

    #[test]
    fn test_send() {
        let n = list_from([1i,2,3]);
        spawn(proc() {
            check_links(&n);
            assert_eq!(&[&1,&2,&3], n.iter().collect::<Vec<&int>>().as_slice());
        });
    }

    #[test]
    fn test_eq() {
        let mut n: DList<u8> = list_from([]);
        let mut m = list_from([]);
        assert!(n == m);
        n.push_front(1);
        assert!(n != m);
        m.push_back(1);
        assert!(n == m);

        let n = list_from([2i,3,4]);
        let m = list_from([1i,2,3]);
        assert!(n != m);
    }

    #[test]
    fn test_hash() {
      let mut x = DList::new();
      let mut y = DList::new();

      assert!(hash::hash(&x) == hash::hash(&y));

      x.push_back(1i);
      x.push_back(2);
      x.push_back(3);

      y.push_front(3i);
      y.push_front(2);
      y.push_front(1);

      assert!(hash::hash(&x) == hash::hash(&y));
    }

    #[test]
    fn test_ord() {
        let n: DList<int> = list_from([]);
        let m = list_from([1i,2,3]);
        assert!(n < m);
        assert!(m > n);
        assert!(n <= n);
        assert!(n >= n);
    }

    #[test]
    fn test_ord_nan() {
        let nan = 0.0f64/0.0;
        let n = list_from([nan]);
        let m = list_from([nan]);
        assert!(!(n < m));
        assert!(!(n > m));
        assert!(!(n <= m));
        assert!(!(n >= m));

        let n = list_from([nan]);
        let one = list_from([1.0f64]);
        assert!(!(n < one));
        assert!(!(n > one));
        assert!(!(n <= one));
        assert!(!(n >= one));

        let u = list_from([1.0f64,2.0,nan]);
        let v = list_from([1.0f64,2.0,3.0]);
        assert!(!(u < v));
        assert!(!(u > v));
        assert!(!(u <= v));
        assert!(!(u >= v));

        let s = list_from([1.0f64,2.0,4.0,2.0]);
        let t = list_from([1.0f64,2.0,3.0,2.0]);
        assert!(!(s < t));
        assert!(s > one);
        assert!(!(s <= one));
        assert!(s >= one);
    }

    #[test]
    fn test_fuzz() {
        for _ in range(0u, 25) {
            fuzz_test(3);
            fuzz_test(16);
            fuzz_test(189);
        }
    }

    #[test]
    fn test_show() {
        let list: DList<int> = range(0i, 10).collect();
        assert!(list.to_string().as_slice() == "[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]");

        let list: DList<&str> = vec!["just", "one", "test", "more"].iter()
                                                                   .map(|&s| s)
                                                                   .collect();
        assert!(list.to_string().as_slice() == "[just, one, test, more]");
    }

    #[cfg(test)]
    fn fuzz_test(sz: int) {
        let mut m: DList<int> = DList::new();
        let mut v = vec![];
        for i in range(0, sz) {
            check_links(&m);
            let r: u8 = rand::random();
            match r % 6 {
                0 => {
                    m.pop_back();
                    v.pop();
                }
                1 => {
                    m.pop_front();
                    v.shift();
                }
                2 | 4 =>  {
                    m.push_front(-i);
                    v.unshift(-i);
                }
                3 | 5 | _ => {
                    m.push_back(i);
                    v.push(i);
                }
            }
        }

        check_links(&m);

        let mut i = 0u;
        for (a, &b) in m.move_iter().zip(v.iter()) {
            i += 1;
            assert_eq!(a, b);
        }
        assert_eq!(i, v.len());
    }

    #[bench]
    fn bench_collect_into(b: &mut test::Bencher) {
        let v = &[0i, ..64];
        b.iter(|| {
            let _: DList<int> = v.iter().map(|x| *x).collect();
        })
    }

    #[bench]
    fn bench_push_front(b: &mut test::Bencher) {
        let mut m: DList<int> = DList::new();
        b.iter(|| {
            m.push_front(0);
        })
    }

    #[bench]
    fn bench_push_back(b: &mut test::Bencher) {
        let mut m: DList<int> = DList::new();
        b.iter(|| {
            m.push_back(0);
        })
    }

    #[bench]
    fn bench_push_back_pop_back(b: &mut test::Bencher) {
        let mut m: DList<int> = DList::new();
        b.iter(|| {
            m.push_back(0);
            m.pop_back();
        })
    }

    #[bench]
    fn bench_push_front_pop_front(b: &mut test::Bencher) {
        let mut m: DList<int> = DList::new();
        b.iter(|| {
            m.push_front(0);
            m.pop_front();
        })
    }

    #[bench]
    fn bench_rotate_forward(b: &mut test::Bencher) {
        let mut m: DList<int> = DList::new();
        m.push_front(0i);
        m.push_front(1);
        b.iter(|| {
            m.rotate_forward();
        })
    }

    #[bench]
    fn bench_rotate_backward(b: &mut test::Bencher) {
        let mut m: DList<int> = DList::new();
        m.push_front(0i);
        m.push_front(1);
        b.iter(|| {
            m.rotate_backward();
        })
    }

    #[bench]
    fn bench_iter(b: &mut test::Bencher) {
        let v = &[0i, ..128];
        let m: DList<int> = v.iter().map(|&x|x).collect();
        b.iter(|| {
            assert!(m.iter().count() == 128);
        })
    }
    #[bench]
    fn bench_iter_mut(b: &mut test::Bencher) {
        let v = &[0i, ..128];
        let mut m: DList<int> = v.iter().map(|&x|x).collect();
        b.iter(|| {
            assert!(m.mut_iter().count() == 128);
        })
    }
    #[bench]
    fn bench_iter_rev(b: &mut test::Bencher) {
        let v = &[0i, ..128];
        let m: DList<int> = v.iter().map(|&x|x).collect();
        b.iter(|| {
            assert!(m.iter().rev().count() == 128);
        })
    }
    #[bench]
    fn bench_iter_mut_rev(b: &mut test::Bencher) {
        let v = &[0i, ..128];
        let mut m: DList<int> = v.iter().map(|&x|x).collect();
        b.iter(|| {
            assert!(m.mut_iter().rev().count() == 128);
        })
    }
}
