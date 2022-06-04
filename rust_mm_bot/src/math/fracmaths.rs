use std::ops::{Mul, Div};


//Overflow warning!! Can potentially overflow if two i64 that are two big multiply!!!
//Why do we do this? Converting between f32, u64, i64, is very expensive, and we're fine with slightly lossy multiplications
#[derive(Copy, Clone)]
pub struct SimpleFracTemplate<T>{
    num : T,
    denom : T
}

pub type SimpleFracu32 = SimpleFracTemplate<u32>;

impl<T : Mul<T, Output = T> + Div<T, Output=T> + Copy> Mul<T> for SimpleFracTemplate<T>{
    type Output = T;
    fn mul(self, rhs: T) -> T{
        self.num*rhs/self.denom
    }
}





