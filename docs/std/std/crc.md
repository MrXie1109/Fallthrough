# `std.crc`

CRC checksums: CRC-8, CRC-16/CCITT and CRC-32/ISO-HDLC.

    checked uint = crc.crc32(&data);

Each is the same algorithm with a different width and polynomial: shift the
register, exclusive-or the polynomial when a bit leaves, and start and end
with the value the standard names. The variants here are the ones files and
network protocols use, and they put their parameters in the doc comment
instead of pretending a CRC is a CRC.

## Declarations

### `export fn crc8(data &const Bytes) u8 {`

CRC-8, polynomial 0x07, initial value 0.

```
export fn crc8(data &const Bytes) u8 {
    register u8 = 0 as u8;
    index usize = 0;
    while (index < data.length()) {
        value u8 = data[index] as u8;
        register = (register ^ value) as u8;
        bit int = 0;
        while (bit < 8) {
            if ((register as uint & 0x80u32) != 0u32) {
                register = (((register as uint) << 1) ^ 0x07u32) as u8;
            } else {
                register = ((register as uint) << 1) as u8;
            }
            bit += 1;
        }
        index += 1;
    }
    return register;
}
```

### `export fn crc16(data &const Bytes) u16 {`

CRC-16/CCITT-FALSE, polynomial 0x1021, initial value 0xffff.

```
export fn crc16(data &const Bytes) u16 {
    register u16 = 0xffffu16;
    index usize = 0;
    while (index < data.length()) {
        value u16 = (data[index] as u16) << 8;
        register = (register ^ value) as u16;
        bit int = 0;
        while (bit < 8) {
            if ((register as uint & 0x8000u32) != 0u32) {
                register = (((register as uint) << 1) ^ 0x1021u32) as u16;
            } else {
                register = ((register as uint) << 1) as u16;
            }
            bit += 1;
        }
        index += 1;
    }
    return register;
}
```

### `export fn crc32(data &const Bytes) uint {`

CRC-32/ISO-HDLC, polynomial 0xedb88320, initial value all ones, final
complement -- the CRC that PNG and zip call CRC-32.

```
export fn crc32(data &const Bytes) uint {
    register uint = 0xffffffffu32;
    index usize = 0;
    while (index < data.length()) {
        value uint = data[index] as uint;
        register = register ^ value;
        bit int = 0;
        while (bit < 8) {
            if ((register & 1u32) != 0u32) {
                register = (register >> 1) ^ 0xedb88320u32;
            } else {
                register = register >> 1;
            }
            bit += 1;
        }
        index += 1;
    }
    return register ^ 0xffffffffu32;
}
```
