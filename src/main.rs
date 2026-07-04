use crate::*;

fn main() {
    // Example usage of the library functions
    let data = vec![1u32, 2, 3, 4];
    match sum_of_squares(&data) {
        ComputeResult::Success(v) => println!("Sum of squares: {}", v),
        _ => println!("Computation failed"),
    }
    // Note: GPU function is async; in a real app you would .await it within an async runtime.
}
