#![allow(dead_code)]

// Keep one audited archive ABI kernel across every execution tier. The ordinary
// Jet package is the public authority; this include makes its internal kernel
// available to the compiler seam without adding a codec-crate dependency.
include!("../../../corelib/core.archive/pkgs/archive/src/lib.rs");

#[cfg(test)]
mod foundation_tests {
    use super::*;

    #[test]
    fn hostile_archive_entry_fanout_is_rejected_before_materialization() {
        let entries = (0..=MAX_ENTRIES)
            .map(|index| (format!("entry-{index}"), Vec::new()))
            .collect::<Vec<_>>();
        assert!(zip_write_all(&entries).is_empty());
        assert!(tar_write_all(&entries).is_empty());
    }

    fn pax_path_record(path_len: usize) -> Vec<u8> {
        let mut length = path_len + 7;
        loop {
            let next = path_len + length.to_string().len() + 7;
            if next == length {
                break;
            }
            length = next;
        }
        let mut record = format!("{length} path=").into_bytes();
        record.extend(std::iter::repeat(b'p').take(path_len));
        record.push(b'\n');
        assert_eq!(record.len(), length);
        record
    }

    fn stored_zip(name_len: usize, data_len: usize) -> Vec<u8> {
        let name = vec![b'n'; name_len];
        let data = vec![b'd'; data_len];
        let crc = crc32(&data);
        let local_size = 30 + name_len + data_len;
        let central_size = 46 + name_len;
        let mut archive = Vec::with_capacity(local_size + central_size + 22);
        put_u32(&mut archive, 0x0403_4b50);
        put_u16(&mut archive, 20);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 0);
        put_u32(&mut archive, crc);
        put_u32(&mut archive, data_len as u32);
        put_u32(&mut archive, data_len as u32);
        put_u16(&mut archive, name_len as u16);
        put_u16(&mut archive, 0);
        archive.extend_from_slice(&name);
        archive.extend_from_slice(&data);
        let central_offset = archive.len() as u32;
        put_u32(&mut archive, 0x0201_4b50);
        put_u16(&mut archive, 20);
        put_u16(&mut archive, 20);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 0);
        put_u32(&mut archive, crc);
        put_u32(&mut archive, data_len as u32);
        put_u32(&mut archive, data_len as u32);
        put_u16(&mut archive, name_len as u16);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 0);
        put_u32(&mut archive, 0);
        put_u32(&mut archive, 0);
        archive.extend_from_slice(&name);
        put_u32(&mut archive, 0x0605_4b50);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 0);
        put_u16(&mut archive, 1);
        put_u16(&mut archive, 1);
        put_u32(&mut archive, central_size as u32);
        put_u32(&mut archive, central_offset);
        put_u16(&mut archive, 0);
        archive
    }

    #[test]
    fn hostile_archive_retained_tar_names_share_materialization_budget() {
        let retained_name_len = TAR_BLOCK * 8;
        for kind in [b'L', b'x'] {
            let metadata = if kind == b'L' {
                vec![b'n'; retained_name_len]
            } else {
                pax_path_record(retained_name_len)
            };
            let data_len = MAX_OUTPUT - retained_name_len + 1;
            let archive_size = tar_entry_wire_len(metadata.len())
                .unwrap()
                .checked_add(tar_entry_wire_len(data_len).unwrap())
                .unwrap()
                .checked_add(TAR_BLOCK * 2)
                .unwrap();
            let mut archive = Vec::with_capacity(archive_size);
            append_tar_entry(&mut archive, "././#LongLink", &metadata, kind);
            let data = vec![b'd'; data_len];
            append_tar_entry(&mut archive, "payload", &data, b'0');
            archive.extend_from_slice(&[0; TAR_BLOCK * 2]);
            assert!(
                tar_read_all(&archive).is_empty(),
                "retained TAR name kind {kind:?} must count against materialization budget"
            );
        }
    }

    #[test]
    fn hostile_zip_name_copies_share_materialization_budget() {
        let name_len = TAR_BLOCK;
        let data_len = MAX_OUTPUT - name_len + 1;
        assert!(zip_read_all(&stored_zip(name_len, data_len)).is_none());
    }

    #[test]
    fn long_tar_name_below_budget_preserves_format_semantics() {
        let name = "nested/".repeat(40) + "file.txt";
        let data = b"payload".to_vec();
        let archive = tar_write_all(&[(name.clone(), data.clone())]);
        assert_eq!(tar_read_all(&archive), vec![(name, data)]);
    }

    #[test]
    fn hostile_zip_name_copies_are_rejected_before_output_allocation() {
        let name = "n".repeat(u16::MAX as usize);
        let entries = (0..1024)
            .map(|_| (name.clone(), Vec::new()))
            .collect::<Vec<_>>();
        assert!(zip_write_all(&entries).is_empty());
    }
}
