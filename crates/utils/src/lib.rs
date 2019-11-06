pub mod io {
    use std::fs;

    pub fn read_file(file: fs::File) -> Result<String, std::io::Error> {
        use std::io::Read;
        let mut buffered_reader = std::io::BufReader::new(file);
        let mut contents = String::new();
        buffered_reader.read_to_string(&mut contents)?;
        Ok(contents)
    }

}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
