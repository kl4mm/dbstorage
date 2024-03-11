use std::{
    cmp::Ordering::{self, *},
    mem::size_of,
    ops::Range,
};

use bytes::{BufMut, BytesMut};

use crate::{
    btree::slot::Increment,
    catalog::{Column, Schema, Type},
    page::PageId,
    storable::Storable,
};

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    TinyInt(i8),
    Bool(bool),
    Int(i32),
    BigInt(i64),
    Varchar(String),
}

impl Value {
    pub fn from(column: &Column, data: &[u8]) -> Value {
        let data = match column.ty {
            Type::Varchar => {
                // First two bytes is the offset
                let offset =
                    u16::from_be_bytes(data[column.offset..column.offset + 2].try_into().unwrap())
                        as usize;

                // Second two bytes is the size
                let size = u16::from_be_bytes(
                    data[column.offset + 2..column.offset + 4]
                        .try_into()
                        .unwrap(),
                ) as usize;

                assert!(offset + size <= data.len());

                &data[offset..offset + size]
            }
            _ => {
                assert!(column.offset + column.size() <= data.len());
                &data[column.offset..column.offset + column.size()]
            }
        };

        match column.ty {
            Type::TinyInt => {
                assert_eq!(data.len(), size_of::<i8>());
                Value::TinyInt(i8::from_be_bytes(data.try_into().unwrap()))
            }
            Type::Bool => {
                assert_eq!(data.len(), size_of::<bool>());
                Value::Bool(u8::from_be_bytes(data.try_into().unwrap()) > 0)
            }
            Type::Int => {
                assert_eq!(data.len(), size_of::<i32>());
                Value::Int(i32::from_be_bytes(data.try_into().unwrap()))
            }
            Type::BigInt => {
                assert_eq!(data.len(), size_of::<i64>());
                Value::BigInt(i64::from_be_bytes(data.try_into().unwrap()))
            }
            Type::Varchar => {
                let str = std::str::from_utf8(data).expect("todo");
                Value::Varchar(str.into())
            }
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::TinyInt(v) => write!(f, "{}", v),
            Value::Bool(v) => write!(f, "{}", v),
            Value::Int(v) => write!(f, "{}", v),
            Value::BigInt(v) => write!(f, "{}", v),
            Value::Varchar(v) => write!(f, "{}", v),
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone, PartialOrd, Eq, Ord)]
pub struct RId {
    pub page_id: PageId,
    pub slot_id: u32,
}

// TODO
impl Storable for RId {
    const SIZE: usize = 0;

    type ByteArray = [u8; 0];

    fn into_bytes(self) -> Self::ByteArray {
        todo!()
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        todo!()
    }

    fn write_to(&self, dst: &mut [u8], pos: usize) {
        todo!()
    }
}

#[derive(Debug, PartialEq, Clone, PartialOrd, Eq, Ord)]
pub struct Tuple {
    pub rid: RId,
    pub data: BytesMut,
}

impl Increment for Tuple {
    fn increment(&mut self) {
        todo!()
    }

    fn next(&self) -> Self {
        todo!()
    }
}

// TODO
impl Storable for Tuple {
    const SIZE: usize = 0;

    type ByteArray = [u8; 0];

    fn into_bytes(self) -> Self::ByteArray {
        todo!()
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        todo!()
    }

    fn write_to(&self, dst: &mut [u8], pos: usize) {
        todo!()
    }
}

impl Tuple {
    pub fn get_value(&self, column: &Column) -> Value {
        Value::from(&column, &self.data)
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct TupleMeta {
    pub deleted: bool,
}

impl From<&[u8]> for TupleMeta {
    fn from(value: &[u8]) -> Self {
        let deleted = u8::from_be_bytes(value[0..1].try_into().unwrap()) > 1;

        Self { deleted }
    }
}

pub const OFFSET: Range<usize> = 0..4;
pub const LEN: Range<usize> = 4..8;
pub const META: Range<usize> = 8..Slot::SIZE;

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct Slot {
    pub offset: u32,
    pub len: u32,
    pub meta: TupleMeta,
}

impl From<&[u8]> for Slot {
    fn from(buf: &[u8]) -> Self {
        let offset = u32::from_be_bytes(buf[OFFSET].try_into().unwrap());
        let len = u32::from_be_bytes(buf[LEN].try_into().unwrap());
        let meta = TupleMeta::from(&buf[META]);

        Self { offset, len, meta }
    }
}

impl Slot {
    pub const SIZE: usize = 9;
}

pub type TupleInfoBuf = [u8; Slot::SIZE];
impl From<&Slot> for TupleInfoBuf {
    fn from(value: &Slot) -> Self {
        let mut ret = [0; Slot::SIZE];

        ret[OFFSET].copy_from_slice(&value.offset.to_be_bytes());
        ret[LEN].copy_from_slice(&value.len.to_be_bytes());
        ret[META].copy_from_slice(&[value.meta.deleted as u8]);

        ret
    }
}

pub struct Comparand<'a, 'b>(&'a Schema, &'b Tuple);

impl<'a, 'b> PartialEq for Comparand<'a, 'b> {
    fn eq(&self, other: &Self) -> bool {
        self.1.data.eq(&other.1.data)
    }
}

impl<'a, 'b> Eq for Comparand<'a, 'b> {}

impl<'a, 'b> PartialOrd for Comparand<'a, 'b> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a, 'b> Ord for Comparand<'a, 'b> {
    fn cmp(&self, other: &Self) -> Ordering {
        for (_, col) in self.0.iter().enumerate() {
            let lhs = self.1.get_value(col);
            let rhs = other.1.get_value(col);

            match lhs.cmp(&rhs) {
                Less => return Less,
                Greater => return Greater,
                _ => {}
            }
        }

        Equal
    }
}

struct Variable {
    data: BytesMut,
    offset_offset: usize,
}

#[derive(Default)]
pub struct TupleBuilder {
    data: BytesMut,
    variable: Vec<Variable>,
}

impl TupleBuilder {
    pub fn new() -> Self {
        Self {
            data: BytesMut::new(),
            ..Default::default()
        }
    }

    pub fn with_capacity(size: usize) -> Self {
        Self {
            data: BytesMut::with_capacity(size),
            ..Default::default()
        }
    }

    pub fn add(mut self, v: &Value) -> Self {
        match v {
            Value::TinyInt(v) => self.data.put(&i8::to_be_bytes(*v)[..]),
            Value::Bool(v) => self.data.put(&u8::to_be_bytes(if *v { 1 } else { 0 })[..]),
            Value::Int(v) => self.data.put(&i32::to_be_bytes(*v)[..]),
            Value::BigInt(v) => self.data.put(&i64::to_be_bytes(*v)[..]),
            Value::Varchar(v) => {
                let offset = self.data.len();

                // First two bytes is the offset, which we won't know until the build()
                // Second two bytes is the length
                self.data.resize(offset + 4, 0);
                self.data[offset + 2..offset + 4]
                    .copy_from_slice(&u16::to_be_bytes(v.len() as u16));

                self.variable.push(Variable {
                    data: BytesMut::from(&v[..]),
                    offset_offset: offset,
                });
            }
        };

        self
    }

    pub fn build(mut self) -> BytesMut {
        for Variable {
            data,
            offset_offset,
        } in self.variable
        {
            let offset = self.data.len();

            // Update offset
            self.data[offset_offset..offset_offset + 2]
                .copy_from_slice(&u16::to_be_bytes(offset as u16));

            // Write variable length data to end of tuple
            self.data.put(data);
        }

        self.data
    }
}

#[cfg(test)]
mod test {
    use bytes::BytesMut;
    use std::cmp::Ordering::{self, *};

    use crate::{
        catalog::{Column, Schema, Type},
        table::tuple::{Comparand, RId, Tuple, TupleBuilder, Value},
    };

    #[test]
    fn test_comparator() {
        let rid = RId {
            page_id: 0,
            slot_id: 0,
        };

        struct Test {
            schema: Schema,
            lhs: BytesMut,
            rhs: BytesMut,
            want: Ordering,
        }

        let tcs = [
            Test {
                schema: Schema::new(vec![
                    Column {
                        name: "col_a".into(),
                        ty: Type::Int,
                        offset: 0,
                    },
                    Column {
                        name: "col_b".into(),
                        ty: Type::Bool,
                        offset: 4,
                    },
                    Column {
                        name: "col_c".into(),
                        ty: Type::BigInt,
                        offset: 5,
                    },
                ]),
                lhs: TupleBuilder::new()
                    .add(&Value::Int(4))
                    .add(&Value::Bool(false))
                    .add(&Value::BigInt(100))
                    .build(),
                rhs: TupleBuilder::new()
                    .add(&Value::Int(4))
                    .add(&Value::Bool(false))
                    .add(&Value::BigInt(100))
                    .build(),
                want: Equal,
            },
            Test {
                schema: Schema::new(vec![
                    Column {
                        name: "col_a".into(),
                        ty: Type::Int,
                        offset: 0,
                    },
                    Column {
                        name: "col_b".into(),
                        ty: Type::Bool,
                        offset: 4,
                    },
                    Column {
                        name: "col_c".into(),
                        ty: Type::BigInt,
                        offset: 5,
                    },
                ]),
                lhs: TupleBuilder::new()
                    .add(&Value::Int(4))
                    .add(&Value::Bool(true))
                    .add(&Value::BigInt(100))
                    .build(),
                rhs: TupleBuilder::new()
                    .add(&Value::Int(4))
                    .add(&Value::Bool(false))
                    .add(&Value::BigInt(100))
                    .build(),
                want: Greater,
            },
            Test {
                schema: Schema::new(vec![
                    Column {
                        name: "col_a".into(),
                        ty: Type::Int,
                        offset: 0,
                    },
                    Column {
                        name: "col_b".into(),
                        ty: Type::Bool,
                        offset: 4,
                    },
                    Column {
                        name: "col_c".into(),
                        ty: Type::BigInt,
                        offset: 5,
                    },
                ]),
                lhs: TupleBuilder::new()
                    .add(&Value::Int(4))
                    .add(&Value::Bool(false))
                    .add(&Value::BigInt(90))
                    .build(),
                rhs: TupleBuilder::new()
                    .add(&Value::Int(4))
                    .add(&Value::Bool(false))
                    .add(&Value::BigInt(100))
                    .build(),
                want: Less,
            },
            Test {
                schema: Schema::new(vec![
                    Column {
                        name: "col_a".into(),
                        ty: Type::TinyInt,
                        offset: 0,
                    },
                    Column {
                        name: "col_b".into(),
                        ty: Type::Varchar,
                        offset: 1,
                    },
                ]),
                lhs: TupleBuilder::new()
                    .add(&Value::TinyInt(1))
                    .add(&Value::Varchar("Column".into()))
                    .build(),
                rhs: TupleBuilder::new()
                    .add(&Value::TinyInt(1))
                    .add(&Value::Varchar("Column".into()))
                    .build(),
                want: Equal,
            },
            Test {
                schema: Schema::new(vec![
                    Column {
                        name: "col_a".into(),
                        ty: Type::Varchar,
                        offset: 0,
                    },
                    Column {
                        name: "col_b".into(),
                        ty: Type::TinyInt,
                        offset: 255 + 2,
                    },
                ]),
                lhs: TupleBuilder::new()
                    .add(&Value::Varchar("Column A".into()))
                    .add(&Value::TinyInt(1))
                    .build(),
                rhs: TupleBuilder::new()
                    .add(&Value::Varchar("Column B".into()))
                    .add(&Value::TinyInt(1))
                    .build(),
                want: Less,
            },
            Test {
                schema: Schema::new(vec![
                    Column {
                        name: "col_a".into(),
                        ty: Type::Varchar,
                        offset: 0,
                    },
                    Column {
                        name: "col_b".into(),
                        ty: Type::TinyInt,
                        offset: 255 + 2,
                    },
                ]),
                lhs: TupleBuilder::new()
                    .add(&Value::Varchar("Column A".into()))
                    .add(&Value::TinyInt(1))
                    .build(),
                rhs: TupleBuilder::new()
                    .add(&Value::Varchar("Column".into()))
                    .add(&Value::TinyInt(1))
                    .build(),
                want: Greater,
            },
        ];

        for Test {
            schema,
            lhs,
            rhs,
            want,
        } in tcs
        {
            let lhs = Tuple { rid, data: lhs };
            let rhs = Tuple { rid, data: rhs };

            let have = Comparand(&schema, &lhs).cmp(&Comparand(&schema, &rhs));
            assert_eq!(want, have);
        }
    }
}
