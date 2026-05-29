#![strict]

struct Employee {
    name: String,
    dept: String,
    salary: u32,
}

fn make_employee(name: &str, dept: &str, salary: u32) -> Employee {
    Employee {
        name: name.to_string(),
        dept: dept.to_string(),
        salary,
    }
}

fn add_employee(roster: &mut Vec<Employee>, e: Employee) {
    roster.push(e);
}

fn total_for_dept(roster: &[Employee], dept: &str) -> u32 {
    roster.iter()
          .filter(|e| e.dept == dept)
          .map(|e| e.salary)
          .sum()
}

fn main() {
    let mut roster: Vec<Employee> = Vec::new();

    add_employee(&mut roster, make_employee("Alice", "Eng", 90000));
    add_employee(&mut roster, make_employee("Bob", "Eng", 85000));
    add_employee(&mut roster, make_employee("Carol", "Sales", 70000));
    add_employee(&mut roster, make_employee("Dave", "Sales", 72000));
    add_employee(&mut roster, make_employee("Eve", "Ops", 80000));

    let eng_total = total_for_dept(&roster, "Eng");
    println!("Eng total: {}", eng_total);

    let sales_total = total_for_dept(&roster, "Sales");
    println!("Sales total: {}", sales_total);
}