"""Read-only ext4 primitives for the sealed MAC.3 arm64 root disk.

This module intentionally starts with the smallest independently testable
surface: the exact sealed superblock geometry and extent-leaf decoding.  It
does not mount, extract, repair, or write an image.
"""

from __future__ import annotations

import collections
import dataclasses
import hashlib
import struct
from typing import Callable, Dict, List, Mapping


SEALED_IMAGE_BYTES = 2_035_625_984
SEALED_BLOCKS_COUNT = 496_979
SEALED_BLOCKS_PER_GROUP = 32_768
SEALED_INODES_COUNT = 22_528
SEALED_INODES_PER_GROUP = 1_408
SEALED_FIRST_DATA_BLOCK = 0
SEALED_LOG_CLUSTER_SIZE = 2
SEALED_CLUSTERS_PER_GROUP = 32_768
SEALED_UUID = bytes.fromhex("00000000000040008000000000000001")
SEALED_FEATURE_COMPAT = 0x3C
SEALED_FEATURE_INCOMPAT = 0x2C2
SEALED_FEATURE_RO_COMPAT = 0x46B
EXT4_MAGIC = 0xEF53
EXT4_VALID_FS = 0x0001
EXTENT_MAGIC = 0xF30A


class RefusedError(RuntimeError):
    """Raised when bytes do not describe the one sealed ext4 shape."""


@dataclasses.dataclass(frozen=True)
class Geometry:
    block_bytes: int
    blocks_count: int
    blocks_per_group: int
    group_count: int
    inodes_count: int
    inodes_per_group: int
    inode_bytes: int
    descriptor_bytes: int
    first_data_block: int
    log_cluster_size: int
    clusters_per_group: int
    uuid: bytes
    superblock_checksum: int
    feature_compat: int
    feature_incompat: int
    feature_ro_compat: int
    creator_os: int
    first_nonreserved_inode: int
    journal_inode: int
    reserved_gdt_blocks: int
    last_orphan: int
    checksum_type: int
    checksum_seed_field: int


@dataclasses.dataclass(frozen=True)
class Extent:
    logical: int
    length: int
    physical: int
    unwritten: bool


@dataclasses.dataclass(frozen=True)
class ExtentIndex:
    logical: int
    child_physical: int


@dataclasses.dataclass(frozen=True)
class GroupDescriptor:
    group: int
    block_bitmap: int
    inode_bitmap: int
    inode_table: int
    free_blocks: int
    free_inodes: int
    used_directories: int
    flags: int
    inode_table_unused: int
    block_bitmap_checksum: int
    inode_bitmap_checksum: int
    descriptor_checksum: int


@dataclasses.dataclass(frozen=True)
class AllocationMap:
    inodes_by_group: Dict[int, List[int]]
    blocks: frozenset[int]


@dataclasses.dataclass(frozen=True)
class Inode:
    number: int
    kind: str
    mode: int
    uid: int
    gid: int
    size: int
    links: int
    flags: int
    generation: int
    checksum: int
    checksum_bits: int
    checksum_seed: int
    extents: tuple[Extent, ...]
    raw_block_map: bytes
    blocks_512: int
    fast_symlink_target: bytes | None
    extent_tree_blocks: tuple[int, ...]


@dataclasses.dataclass(frozen=True)
class DirectoryEntry:
    inode: int
    name: str
    kind: str
    record_bytes: int


@dataclasses.dataclass(frozen=True)
class BlockOwner:
    classification: str
    inode: int | None
    logical: int | None


@dataclasses.dataclass(frozen=True)
class PathGraph:
    path_to_inode: Dict[str, int]
    paths_by_inode: Dict[int, tuple[str, ...]]


@dataclasses.dataclass(frozen=True)
class MappedRawHit:
    marker: str
    raw_offset: int
    needle_bytes: int
    inode: int
    file_offset: int
    paths: tuple[str, ...]
    physical_blocks: tuple[int, ...]
    block_sha256s: tuple[str, ...]


SEALED_OWNER_COUNTS = {
    "file-data": 422_636,
    "directory-data": 1_766,
    "symlink-data": 7,
    "journal": 8_192,
    "extent-metadata": 24,
    "allocation-metadata": 1_440,
    "super-gdt": 12,
    "resize-metadata": 1_453,
}


def _u16(value: bytes, offset: int) -> int:
    return struct.unpack_from("<H", value, offset)[0]


def _u32(value: bytes, offset: int) -> int:
    return struct.unpack_from("<I", value, offset)[0]


def _crc32c_table() -> tuple[int, ...]:
    polynomial = 0x82F63B78
    rows = []
    for octet in range(256):
        crc = octet
        for _ in range(8):
            crc = (crc >> 1) ^ polynomial if crc & 1 else crc >> 1
        rows.append(crc & 0xFFFFFFFF)
    return tuple(rows)


_CRC32C_TABLE = _crc32c_table()


def _crc32c(seed: int, value: bytes) -> int:
    crc = seed
    for octet in value:
        crc = _CRC32C_TABLE[(crc ^ octet) & 0xFF] ^ (crc >> 8)
    return crc & 0xFFFFFFFF


def parse_superblock(block: bytes, image_size: int) -> Geometry:
    """Parse and exact-match the sealed image's 1,024-byte superblock."""

    if not isinstance(block, bytes) or len(block) != 1_024:
        raise RefusedError("ext4 superblock must be exactly 1024 bytes")
    if image_size != SEALED_IMAGE_BYTES:
        raise RefusedError("image size differs from the sealed root disk")
    if _u16(block, 0x38) != EXT4_MAGIC:
        raise RefusedError("bad ext4 magic")
    state = _u16(block, 0x3A)
    if state != EXT4_VALID_FS:
        raise RefusedError("sealed filesystem is not clean")

    log_block_bytes = _u32(block, 0x18)
    if log_block_bytes > 6:
        raise RefusedError("unsupported ext4 block size")
    block_bytes = 1_024 << log_block_bytes
    blocks_count = _u32(block, 0x04) | (_u32(block, 0x150) << 32)
    first_data_block = _u32(block, 0x14)
    log_cluster_size = _u32(block, 0x1C)
    blocks_per_group = _u32(block, 0x20)
    clusters_per_group = _u32(block, 0x24)
    inodes_count = _u32(block, 0x00)
    inodes_per_group = _u32(block, 0x28)
    inode_bytes = _u16(block, 0x58)
    descriptor_bytes = _u16(block, 0xFE)
    feature_compat = _u32(block, 0x5C)
    feature_incompat = _u32(block, 0x60)
    feature_ro_compat = _u32(block, 0x64)
    uuid = block[0x68:0x78]
    superblock_checksum = _u32(block, 0x3FC)
    creator_os = _u32(block, 0x48)
    first_nonreserved_inode = _u32(block, 0x54)
    reserved_gdt_blocks = _u16(block, 0xCE)
    journal_inode = _u32(block, 0xE0)
    last_orphan = _u32(block, 0xE8)
    checksum_type = block[0x175]
    checksum_seed_field = _u32(block, 0x270)

    if (feature_compat, feature_incompat, feature_ro_compat) != (
        SEALED_FEATURE_COMPAT,
        SEALED_FEATURE_INCOMPAT,
        SEALED_FEATURE_RO_COMPAT,
    ):
        raise RefusedError("ext4 feature mask differs from the sealed image")
    if (
        block_bytes != 4_096
        or blocks_count != SEALED_BLOCKS_COUNT
        or first_data_block != SEALED_FIRST_DATA_BLOCK
        or log_cluster_size != SEALED_LOG_CLUSTER_SIZE
        or blocks_per_group != SEALED_BLOCKS_PER_GROUP
        or clusters_per_group != SEALED_CLUSTERS_PER_GROUP
        or inodes_count != SEALED_INODES_COUNT
        or inodes_per_group != SEALED_INODES_PER_GROUP
        or inode_bytes != 256
        or descriptor_bytes != 64
        or uuid != SEALED_UUID
        or creator_os != 0
        or first_nonreserved_inode != 11
        or reserved_gdt_blocks != 242
        or journal_inode != 8
        or last_orphan != 0
        or checksum_type != 1
        or checksum_seed_field != 0
        or superblock_checksum != _crc32c(0xFFFFFFFF, block[:0x3FC])
        or blocks_count * block_bytes != image_size
    ):
        raise RefusedError("ext4 geometry differs from the sealed image")
    group_count = (
        blocks_count - first_data_block + blocks_per_group - 1
    ) // blocks_per_group
    if group_count != 16 or group_count * inodes_per_group != inodes_count:
        raise RefusedError("ext4 group geometry differs from the sealed image")
    return Geometry(
        block_bytes=block_bytes,
        blocks_count=blocks_count,
        blocks_per_group=blocks_per_group,
        group_count=group_count,
        inodes_count=inodes_count,
        inodes_per_group=inodes_per_group,
        inode_bytes=inode_bytes,
        descriptor_bytes=descriptor_bytes,
        first_data_block=first_data_block,
        log_cluster_size=log_cluster_size,
        clusters_per_group=clusters_per_group,
        uuid=uuid,
        superblock_checksum=superblock_checksum,
        feature_compat=feature_compat,
        feature_incompat=feature_incompat,
        feature_ro_compat=feature_ro_compat,
        creator_os=creator_os,
        first_nonreserved_inode=first_nonreserved_inode,
        journal_inode=journal_inode,
        reserved_gdt_blocks=reserved_gdt_blocks,
        last_orphan=last_orphan,
        checksum_type=checksum_type,
        checksum_seed_field=checksum_seed_field,
    )


def filesystem_checksum_seed(geometry: Geometry) -> int:
    """Return the metadata-csum seed used by this non-CSUM_SEED image."""

    if geometry.uuid != SEALED_UUID or geometry.feature_incompat & 0x2000:
        raise RefusedError("the sealed image must derive its checksum seed from UUID")
    return _crc32c(0xFFFFFFFF, geometry.uuid)


def _combine_low_high(value: bytes, low_offset: int, high_offset: int) -> int:
    return _u32(value, low_offset) | (_u32(value, high_offset) << 32)


def parse_group_descriptors(
    table: bytes, *, geometry: Geometry
) -> List[GroupDescriptor]:
    """Validate and decode the one complete primary group-descriptor table."""

    exact_bytes = geometry.group_count * geometry.descriptor_bytes
    if not isinstance(table, bytes) or len(table) != exact_bytes:
        raise RefusedError("group descriptor table has the wrong size")
    checksum_seed = filesystem_checksum_seed(geometry)
    inode_table_blocks = (
        geometry.inodes_per_group * geometry.inode_bytes
        + geometry.block_bytes
        - 1
    ) // geometry.block_bytes
    descriptors = []
    for group in range(geometry.group_count):
        start = group * geometry.descriptor_bytes
        raw = table[start : start + geometry.descriptor_bytes]
        stored_checksum = _u16(raw, 0x1E)
        checksum_input = bytearray(raw)
        struct.pack_into("<H", checksum_input, 0x1E, 0)
        checksum = _crc32c(
            _crc32c(checksum_seed, struct.pack("<I", group)),
            bytes(checksum_input),
        )
        if stored_checksum != checksum & 0xFFFF:
            raise RefusedError("group descriptor checksum mismatch")

        block_bitmap = _combine_low_high(raw, 0x00, 0x20)
        inode_bitmap = _combine_low_high(raw, 0x04, 0x24)
        inode_table = _combine_low_high(raw, 0x08, 0x28)
        free_blocks = _u16(raw, 0x0C) | (_u16(raw, 0x2C) << 16)
        free_inodes = _u16(raw, 0x0E) | (_u16(raw, 0x2E) << 16)
        used_directories = _u16(raw, 0x10) | (_u16(raw, 0x30) << 16)
        flags = _u16(raw, 0x12)
        inode_table_unused = _u16(raw, 0x1C) | (_u16(raw, 0x32) << 16)
        block_bitmap_checksum = _u16(raw, 0x18) | (_u16(raw, 0x38) << 16)
        inode_bitmap_checksum = _u16(raw, 0x1A) | (_u16(raw, 0x3A) << 16)

        valid_blocks = min(
            geometry.blocks_per_group,
            geometry.blocks_count - group * geometry.blocks_per_group,
        )
        if (
            flags & ~0x7
            or not 0 < block_bitmap < geometry.blocks_count
            or not 0 < inode_bitmap < geometry.blocks_count
            or not 0 < inode_table < geometry.blocks_count
            or inode_table + inode_table_blocks > geometry.blocks_count
            or free_blocks > valid_blocks
            or free_inodes > geometry.inodes_per_group
            or used_directories > geometry.inodes_per_group - free_inodes
            or inode_table_unused > free_inodes
            or raw[0x14:0x18] != bytes(4)
            or raw[0x34:0x38] != bytes(4)
            or raw[0x3C:0x40] != bytes(4)
        ):
            raise RefusedError("group descriptor differs from the sealed shape")
        descriptors.append(
            GroupDescriptor(
                group=group,
                block_bitmap=block_bitmap,
                inode_bitmap=inode_bitmap,
                inode_table=inode_table,
                free_blocks=free_blocks,
                free_inodes=free_inodes,
                used_directories=used_directories,
                flags=flags,
                inode_table_unused=inode_table_unused,
                block_bitmap_checksum=block_bitmap_checksum,
                inode_bitmap_checksum=inode_bitmap_checksum,
                descriptor_checksum=stored_checksum,
            )
        )
    return descriptors


def _read_block_exact(read_block: Callable[[int], bytes], block: int) -> bytes:
    try:
        value = read_block(block)
    except Exception as error:
        raise RefusedError("failed to read ext4 metadata block") from error
    if not isinstance(value, bytes) or len(value) != 4_096:
        raise RefusedError("ext4 metadata block has the wrong size")
    return value


def _set_bit_indices(bitmap: bytes, relevant_bits: int) -> List[int]:
    if relevant_bits < 0 or relevant_bits > len(bitmap) * 8:
        raise RefusedError("bitmap range is outside the checksum-bound bytes")
    return [
        index
        for index in range(relevant_bits)
        if bitmap[index // 8] & (1 << (index % 8))
    ]


def verify_allocation_bitmaps(
    descriptors: List[GroupDescriptor],
    *,
    geometry: Geometry,
    read_block: Callable[[int], bytes],
) -> AllocationMap:
    """Verify allocation checksums/counts and return allocated inode numbers."""

    if [row.group for row in descriptors] != list(range(geometry.group_count)):
        raise RefusedError("group descriptor sequence is incomplete")
    checksum_seed = filesystem_checksum_seed(geometry)
    inode_bitmap_bytes = (geometry.inodes_per_group + 7) // 8
    allocated_by_group = {}
    allocated_physical_blocks = set()
    for descriptor in descriptors:
        group = descriptor.group
        valid_blocks = min(
            geometry.blocks_per_group,
            geometry.blocks_count - group * geometry.blocks_per_group,
        )
        block_bitmap = _read_block_exact(read_block, descriptor.block_bitmap)
        inode_bitmap_block = _read_block_exact(read_block, descriptor.inode_bitmap)
        inode_bitmap = inode_bitmap_block[:inode_bitmap_bytes]
        allocated_blocks = _set_bit_indices(block_bitmap, valid_blocks)
        allocated_inodes = _set_bit_indices(
            inode_bitmap, geometry.inodes_per_group
        )
        if descriptor.flags & 0x2:
            if descriptor.block_bitmap_checksum != 0 or allocated_blocks:
                raise RefusedError("uninitialised block bitmap is not empty")
        elif _crc32c(checksum_seed, block_bitmap) != descriptor.block_bitmap_checksum:
            raise RefusedError("block bitmap checksum mismatch")
        if descriptor.flags & 0x1:
            if descriptor.inode_bitmap_checksum != 0 or allocated_inodes:
                raise RefusedError("uninitialised inode bitmap is not empty")
        elif _crc32c(checksum_seed, inode_bitmap) != descriptor.inode_bitmap_checksum:
            raise RefusedError("inode bitmap checksum mismatch")

        if len(allocated_blocks) != valid_blocks - descriptor.free_blocks:
            raise RefusedError("block bitmap count differs from its descriptor")
        if len(allocated_inodes) != geometry.inodes_per_group - descriptor.free_inodes:
            raise RefusedError("inode bitmap count differs from its descriptor")
        allocated_by_group[group] = [
            group * geometry.inodes_per_group + index + 1
            for index in allocated_inodes
        ]
        allocated_physical_blocks.update(
            group * geometry.blocks_per_group + index
            for index in allocated_blocks
        )
    return AllocationMap(
        inodes_by_group=allocated_by_group,
        blocks=frozenset(allocated_physical_blocks),
    )


def inode_checksum_seed(
    geometry: Geometry, *, inode_number: int, generation: int
) -> int:
    if not 1 <= inode_number <= geometry.inodes_count:
        raise RefusedError("inode number is outside the sealed filesystem")
    if not 0 <= generation <= 0xFFFFFFFF:
        raise RefusedError("inode generation is outside uint32")
    seed = _crc32c(
        filesystem_checksum_seed(geometry), struct.pack("<I", inode_number)
    )
    return _crc32c(seed, struct.pack("<I", generation))


def parse_inode(
    raw: bytes,
    *,
    inode_number: int,
    geometry: Geometry,
    read_block: Callable[[int], bytes] | None = None,
) -> Inode:
    """Checksum and decode one allocated inode from the sealed 256-byte table."""

    if not isinstance(raw, bytes) or len(raw) != geometry.inode_bytes:
        raise RefusedError("inode record has the wrong size")
    generation = _u32(raw, 0x64)
    checksum_seed = inode_checksum_seed(
        geometry, inode_number=inode_number, generation=generation
    )
    checksum_input = bytearray(raw)
    stored_low = _u16(raw, 0x7C)
    struct.pack_into("<H", checksum_input, 0x7C, 0)
    extra_isize = _u16(raw, 0x80)
    if extra_isize not in (0, 32):
        raise RefusedError("inode extra size differs from the sealed layouts")
    inline_attribute_start = 0x80 + extra_isize
    if any(raw[inline_attribute_start:]):
        raise RefusedError("sealed image has no inline inode attributes")
    if extra_isize >= 4:
        stored_high = _u16(raw, 0x82)
        struct.pack_into("<H", checksum_input, 0x82, 0)
        checksum_bits = 32
    else:
        stored_high = 0
        checksum_bits = 16
    calculated = _crc32c(checksum_seed, bytes(checksum_input))
    stored = stored_low | (stored_high << 16)
    mask = 0xFFFFFFFF if checksum_bits == 32 else 0xFFFF
    if stored != calculated & mask:
        raise RefusedError("inode checksum mismatch")

    mode = _u16(raw, 0x00)
    file_type = mode & 0xF000
    kinds = {0: "reserved", 0x4000: "directory", 0x8000: "regular", 0xA000: "symlink"}
    if file_type not in kinds:
        raise RefusedError("unsupported inode kind")
    kind = kinds[file_type]
    uid = _u16(raw, 0x02) | (_u16(raw, 0x78) << 16)
    gid = _u16(raw, 0x18) | (_u16(raw, 0x7A) << 16)
    size = _u32(raw, 0x04) | (_u32(raw, 0x6C) << 32)
    links = _u16(raw, 0x1A)
    blocks_512 = _u32(raw, 0x1C) | (_u16(raw, 0x74) << 32)
    file_acl = _u32(raw, 0x68) | (_u16(raw, 0x76) << 32)
    flags = _u32(raw, 0x20)
    raw_block_map = raw[0x28:0x64]
    extents: tuple[Extent, ...] = ()
    fast_symlink_target = None
    extent_tree_blocks: tuple[int, ...] = ()
    if flags & 0x80000:
        depth = _u16(raw_block_map, 6)
        if depth == 0:
            extents = tuple(parse_extent_node(raw_block_map, expected_depth=0))
        elif depth == 1 and read_block is not None:
            extent_tree_blocks = tuple(
                row.child_physical for row in _parse_extent_indices(raw_block_map)
            )
            extents = tuple(
                parse_extent_tree(
                    raw_block_map,
                    read_block=read_block,
                    inode_checksum_seed=checksum_seed,
                )
            )
        else:
            raise RefusedError("inode extent tree cannot be read in this context")
    elif kind == "symlink":
        if size > 60 or blocks_512 != 0 or file_acl != 0:
            raise RefusedError("non-extent symlink differs from the fast form")
        fast_symlink_target = raw_block_map[:size]
    elif inode_number == 7:
        expected_pointers = (0,) * 13 + (1_689, 0)
        if (
            kind != "regular"
            or mode != 0o100600
            or size != 4_299_210_752
            or blocks_512 != 11_624
            or flags != 0
            or struct.unpack("<15I", raw_block_map) != expected_pointers
        ):
            raise RefusedError("resize inode differs from the sealed metadata inode")
        kind = "resize-metadata"
    elif kind != "reserved":
        raise RefusedError("allocated file lacks the sealed extent format")
    if kind == "reserved" and (mode != 0 or size != 0 or links != 0):
        raise RefusedError("reserved inode is not empty")
    if file_acl != 0:
        raise RefusedError("sealed image has no external inode attributes")
    if inode_number == 8:
        observed_extents = [
            (row.logical, row.length, row.physical, row.unwritten)
            for row in extents
        ]
        if (
            kind != "regular"
            or mode != 0o100600
            or size != 33_554_432
            or blocks_512 != 65_536
            or observed_extents != [(0, 8_192, 196_608, False)]
        ):
            raise RefusedError("journal inode differs from the sealed layout")
        kind = "journal"
    return Inode(
        number=inode_number,
        kind=kind,
        mode=mode,
        uid=uid,
        gid=gid,
        size=size,
        links=links,
        flags=flags,
        generation=generation,
        checksum=stored,
        checksum_bits=checksum_bits,
        checksum_seed=checksum_seed,
        extents=extents,
        raw_block_map=raw_block_map,
        blocks_512=blocks_512,
        fast_symlink_target=fast_symlink_target,
        extent_tree_blocks=extent_tree_blocks,
    )


def read_allocated_inodes(
    descriptors: List[GroupDescriptor],
    *,
    allocation: AllocationMap,
    geometry: Geometry,
    read_block: Callable[[int], bytes],
) -> Dict[int, Inode]:
    """Read every allocation-bitmap inode from its checksum-bound table slot."""

    expected_groups = list(range(geometry.group_count))
    if [row.group for row in descriptors] != expected_groups:
        raise RefusedError("group descriptor sequence is incomplete")
    if sorted(allocation.inodes_by_group) != expected_groups:
        raise RefusedError("inode allocation groups are incomplete")
    cached_blocks = {}

    def cached_read(block: int) -> bytes:
        if block not in allocation.blocks:
            raise RefusedError("inode metadata references an unallocated block")
        if block not in cached_blocks:
            cached_blocks[block] = _read_block_exact(read_block, block)
        return cached_blocks[block]

    inodes = {}
    for descriptor in descriptors:
        numbers = allocation.inodes_by_group[descriptor.group]
        if numbers != sorted(set(numbers)):
            raise RefusedError("allocated inode sequence is duplicated or unordered")
        for inode_number in numbers:
            actual_group = (inode_number - 1) // geometry.inodes_per_group
            if actual_group != descriptor.group:
                raise RefusedError("allocated inode is assigned to the wrong group")
            index = (inode_number - 1) % geometry.inodes_per_group
            byte_offset = index * geometry.inode_bytes
            table_block = descriptor.inode_table + byte_offset // geometry.block_bytes
            within_block = byte_offset % geometry.block_bytes
            block = cached_read(table_block)
            raw = block[within_block : within_block + geometry.inode_bytes]
            if len(raw) != geometry.inode_bytes or inode_number in inodes:
                raise RefusedError("allocated inode table slot is incomplete or duplicated")
            inodes[inode_number] = parse_inode(
                raw,
                inode_number=inode_number,
                geometry=geometry,
                read_block=cached_read,
            )
    return inodes


def read_resize_metadata_blocks(
    inode: Inode,
    *,
    read_block: Callable[[int], bytes],
    allocated_blocks: frozenset[int],
) -> frozenset[int]:
    """Validate inode 7's exact double-indirect reserved-GDT ownership."""

    if (
        not isinstance(inode, Inode)
        or inode.number != 7
        or inode.kind != "resize-metadata"
        or struct.unpack("<15I", inode.raw_block_map) != (0,) * 13 + (1_689, 0)
    ):
        raise RefusedError("resize metadata inode identity differs")
    double_indirect = _read_block_exact(read_block, 1_689)
    top = struct.unpack("<1024I", double_indirect)
    expected_indirect = tuple(range(2, 244))
    if top[0] != 0 or top[1:243] != expected_indirect or any(top[243:]):
        raise RefusedError("resize double-indirect table differs")

    owned = {1_689, *expected_indirect}
    for indirect_block in expected_indirect:
        rows = struct.unpack("<1024I", _read_block_exact(read_block, indirect_block))
        expected = tuple(
            group * SEALED_BLOCKS_PER_GROUP + indirect_block
            for group in (1, 3, 5, 7, 9)
        )
        if rows[:5] != expected or any(rows[5:]):
            raise RefusedError("resize reserved-GDT pointer table differs")
        owned.update(expected)
    if len(owned) != 1_453 or not owned <= allocated_blocks:
        raise RefusedError("resize metadata coverage is incomplete or unallocated")
    return frozenset(owned)


def build_block_ownership(
    *,
    geometry: Geometry,
    descriptors: List[GroupDescriptor],
    allocation: AllocationMap,
    inodes: Mapping[int, Inode],
    read_block: Callable[[int], bytes],
    expected_counts: Mapping[str, int] | None = None,
) -> Dict[int, BlockOwner]:
    """Classify every allocated block exactly once or refuse the image."""

    expected = dict(SEALED_OWNER_COUNTS if expected_counts is None else expected_counts)
    if set(expected) != set(SEALED_OWNER_COUNTS) or any(
        not isinstance(value, int) or isinstance(value, bool) or value < 0
        for value in expected.values()
    ):
        raise RefusedError("physical owner count contract is incomplete")
    owners: Dict[int, BlockOwner] = {}

    def claim(
        block: int,
        classification: str,
        *,
        inode: int | None = None,
        logical: int | None = None,
    ) -> None:
        if (
            classification not in expected
            or not isinstance(block, int)
            or isinstance(block, bool)
            or not 0 <= block < geometry.blocks_count
            or block not in allocation.blocks
            or block in owners
        ):
            raise RefusedError("allocated block has no unique physical owner")
        owners[block] = BlockOwner(
            classification=classification, inode=inode, logical=logical
        )

    for group in (0, 1, 3, 5, 7, 9):
        start = group * geometry.blocks_per_group
        claim(start, "super-gdt")
        claim(start + 1, "super-gdt")
    inode_table_blocks = (
        geometry.inodes_per_group * geometry.inode_bytes
        // geometry.block_bytes
    )
    if inode_table_blocks != 88:
        raise RefusedError("sealed inode table block count differs")
    for descriptor in descriptors:
        claim(descriptor.block_bitmap, "allocation-metadata")
        claim(descriptor.inode_bitmap, "allocation-metadata")
        for block in range(
            descriptor.inode_table, descriptor.inode_table + inode_table_blocks
        ):
            claim(block, "allocation-metadata")

    resize_inode = inodes.get(7)
    if resize_inode is None:
        raise RefusedError("resize metadata inode is absent")
    for block in read_resize_metadata_blocks(
        resize_inode,
        read_block=read_block,
        allocated_blocks=allocation.blocks,
    ):
        claim(block, "resize-metadata", inode=7)

    data_classes = {
        "regular": "file-data",
        "directory": "directory-data",
        "symlink": "symlink-data",
        "journal": "journal",
    }
    for inode_number in sorted(inodes):
        inode = inodes[inode_number]
        for leaf_block in inode.extent_tree_blocks:
            claim(leaf_block, "extent-metadata", inode=inode_number)
        if not inode.extents:
            if inode.kind == "regular" and inode.size == 0:
                continue
            if inode.kind not in ("reserved", "resize-metadata", "symlink"):
                raise RefusedError("content inode has no physical extent")
            continue
        classification = data_classes.get(inode.kind)
        if classification is None:
            raise RefusedError("extent inode has an unsupported physical class")
        for extent in inode.extents:
            if extent.unwritten:
                raise RefusedError("sealed image contains an unwritten extent")
            for offset in range(extent.length):
                claim(
                    extent.physical + offset,
                    classification,
                    inode=inode_number,
                    logical=extent.logical + offset,
                )

    if set(owners) != set(allocation.blocks):
        raise RefusedError("allocated blocks are unowned or unexpected")
    observed_counts = collections.Counter(
        owner.classification for owner in owners.values()
    )
    if dict(observed_counts) != {key: value for key, value in expected.items() if value}:
        raise RefusedError("physical owner counts differ from their sealed contract")
    return owners


def _inode_logical_blocks(inode: Inode) -> Dict[int, int]:
    result = {}
    for extent in inode.extents:
        if extent.unwritten:
            raise RefusedError("logical file mapping contains an unwritten extent")
        for offset in range(extent.length):
            logical = extent.logical + offset
            physical = extent.physical + offset
            if logical in result:
                raise RefusedError("logical file block has multiple physical owners")
            result[logical] = physical
    return result


def read_inode_payload(
    inode: Inode,
    read_block: Callable[[int], bytes],
) -> bytes:
    """Read exactly the bytes owned by one content inode, refusing holes.

    The sealed tree contains ordinary ext4 sparse files: a missing logical block
    reads as zero.  Written extents must still stay within ``i_size`` and the
    global owner map has already rejected unwritten extents and overlaps.  Fast
    symlinks are the sole inline-data case.
    """

    if not isinstance(inode, Inode) or inode.kind not in (
        "regular",
        "symlink",
        "journal",
    ):
        raise RefusedError("inode is not readable file content")
    if inode.size < 0:
        raise RefusedError("inode has a negative size")
    if inode.fast_symlink_target is not None:
        if inode.kind != "symlink" or len(inode.fast_symlink_target) != inode.size:
            raise RefusedError("fast symlink payload differs from i_size")
        return inode.fast_symlink_target

    logical_blocks = _inode_logical_blocks(inode)
    required = (inode.size + 4_095) // 4_096
    if any(logical < 0 or logical >= required for logical in logical_blocks):
        raise RefusedError("content inode extent extends past i_size")
    payload = bytearray()
    for logical in range(required):
        physical = logical_blocks.get(logical)
        payload.extend(
            bytes(4_096)
            if physical is None
            else _read_block_exact(read_block, physical)
        )
    return bytes(payload[: inode.size])


def walk_directory_tree(
    *,
    inodes: Mapping[int, Inode],
    owners: Mapping[int, BlockOwner],
    read_block: Callable[[int], bytes],
) -> PathGraph:
    """Walk inode 2 and bind every visible path to checksum-verified metadata."""

    root = inodes.get(2)
    if root is None or root.kind != "directory":
        raise RefusedError("sealed root inode is absent")
    queue = collections.deque([(2, 2, "")])
    visited_directories = set()
    enqueued_directories = {2}
    path_to_inode: Dict[str, int] = {}
    paths_by_inode: Dict[int, List[str]] = collections.defaultdict(list)
    child_directory_counts = collections.Counter()

    while queue:
        inode_number, parent_inode, prefix = queue.popleft()
        if inode_number in visited_directories:
            raise RefusedError("directory graph contains a cycle or second parent")
        visited_directories.add(inode_number)
        inode = inodes.get(inode_number)
        if inode is None or inode.kind != "directory" or inode.size % 4_096:
            raise RefusedError("directory inode shape differs")
        logical_blocks = _inode_logical_blocks(inode)
        expected_logical = set(range(inode.size // 4_096))
        if set(logical_blocks) != expected_logical:
            raise RefusedError("directory data has a hole or extends past i_size")

        entries = []
        for logical in sorted(logical_blocks):
            physical = logical_blocks[logical]
            owner = owners.get(physical)
            if owner != BlockOwner("directory-data", inode_number, logical):
                raise RefusedError("directory block ownership differs")
            entries.extend(
                parse_directory_block(
                    _read_block_exact(read_block, physical),
                    directory_inode=inode,
                )
            )
        dots = [row for row in entries if row.name == "."]
        dotdots = [row for row in entries if row.name == ".."]
        if (
            len(dots) != 1
            or dots[0].inode != inode_number
            or dots[0].kind != "directory"
            or len(dotdots) != 1
            or dotdots[0].inode != parent_inode
            or dotdots[0].kind != "directory"
        ):
            raise RefusedError("directory dot entries differ from the graph")

        local_names = set()
        for row in entries:
            if row.name in (".", ".."):
                continue
            if row.name in local_names:
                raise RefusedError("directory contains a duplicate name")
            local_names.add(row.name)
            child = inodes.get(row.inode)
            if child is None or child.kind != row.kind:
                raise RefusedError("directory type differs from the referenced inode")
            path = "%s/%s" % (prefix, row.name) if prefix else row.name
            if path in path_to_inode:
                raise RefusedError("logical path is duplicated")
            path_to_inode[path] = row.inode
            paths_by_inode[row.inode].append(path)
            if child.kind == "directory":
                if row.inode in enqueued_directories:
                    raise RefusedError("directory has multiple visible parents")
                enqueued_directories.add(row.inode)
                child_directory_counts[inode_number] += 1
                queue.append((row.inode, inode_number, path))

    visible_numbers = {
        number
        for number, inode in inodes.items()
        if inode.kind in ("regular", "directory", "symlink") and number != 2
    }
    if set(paths_by_inode) != visible_numbers:
        raise RefusedError("allocated visible inode is missing from the root graph")
    for number, inode in inodes.items():
        if inode.kind in ("regular", "symlink"):
            if inode.links != len(paths_by_inode.get(number, [])):
                raise RefusedError("file link count differs from visible aliases")
        elif inode.kind == "directory":
            aliases = paths_by_inode.get(number, [])
            if number != 2 and len(aliases) != 1:
                raise RefusedError("directory must have exactly one visible parent")
            expected_links = 2 + child_directory_counts[number]
            if inode.links != expected_links:
                raise RefusedError("directory link count differs from child graph")
    return PathGraph(
        path_to_inode=dict(sorted(path_to_inode.items())),
        paths_by_inode={
            number: tuple(sorted(paths))
            for number, paths in sorted(paths_by_inode.items())
        },
    )


def map_raw_hit(
    *,
    marker: str,
    raw_offset: int,
    needle: bytes,
    image_size: int,
    owners: Mapping[int, BlockOwner],
    inodes: Mapping[int, Inode],
    paths_by_inode: Mapping[int, tuple[str, ...]],
    read_block: Callable[[int], bytes],
) -> MappedRawHit:
    """Map a complete raw marker span into one visible written regular file."""

    if (
        not isinstance(marker, str)
        or not marker
        or not isinstance(raw_offset, int)
        or isinstance(raw_offset, bool)
        or raw_offset < 0
        or not isinstance(needle, bytes)
        or not needle
        or image_size != SEALED_IMAGE_BYTES
        or raw_offset + len(needle) > image_size
    ):
        raise RefusedError("raw marker identity is invalid")
    cursor = raw_offset
    needle_cursor = 0
    selected_inode = None
    first_file_offset = None
    previous_file_end = None
    physical_blocks = []
    block_sha256s = []
    while needle_cursor < len(needle):
        physical_block = cursor // 4_096
        within_block = cursor % 4_096
        take = min(4_096 - within_block, len(needle) - needle_cursor)
        owner = owners.get(physical_block)
        if (
            owner is None
            or owner.classification != "file-data"
            or owner.inode is None
            or owner.logical is None
        ):
            raise RefusedError("raw marker touches non-file or unowned bytes")
        inode = inodes.get(owner.inode)
        paths = paths_by_inode.get(owner.inode)
        file_offset = owner.logical * 4_096 + within_block
        if (
            inode is None
            or inode.kind != "regular"
            or not paths
            or tuple(sorted(set(paths))) != tuple(paths)
            or file_offset + take > inode.size
            or (selected_inode is not None and selected_inode != inode.number)
            or (previous_file_end is not None and previous_file_end != file_offset)
        ):
            raise RefusedError("raw marker has no single contiguous logical file owner")
        block = _read_block_exact(read_block, physical_block)
        if block[within_block : within_block + take] != needle[
            needle_cursor : needle_cursor + take
        ]:
            raise RefusedError("raw marker bytes differ at the mapped owner")
        if selected_inode is None:
            selected_inode = inode.number
            first_file_offset = file_offset
        previous_file_end = file_offset + take
        physical_blocks.append(physical_block)
        block_sha256s.append(hashlib.sha256(block).hexdigest())
        cursor += take
        needle_cursor += take
    assert selected_inode is not None and first_file_offset is not None
    return MappedRawHit(
        marker=marker,
        raw_offset=raw_offset,
        needle_bytes=len(needle),
        inode=selected_inode,
        file_offset=first_file_offset,
        paths=paths_by_inode[selected_inode],
        physical_blocks=tuple(physical_blocks),
        block_sha256s=tuple(block_sha256s),
    )


def parse_directory_block(
    block: bytes, *, directory_inode: Inode
) -> List[DirectoryEntry]:
    """Verify and decode one checksum-tailed classic directory block."""

    if (
        not isinstance(block, bytes)
        or len(block) != 4_096
        or not isinstance(directory_inode, Inode)
        or directory_inode.kind != "directory"
        or directory_inode.flags & 0x1000
    ):
        raise RefusedError("unsupported directory block context")
    reserved, tail_length, tail_name_length, tail_type, stored_checksum = (
        struct.unpack_from("<IHBBI", block, 4_084)
    )
    if (
        reserved != 0
        or tail_length != 12
        or tail_name_length != 0
        or tail_type != 0xDE
        or stored_checksum
        != _crc32c(directory_inode.checksum_seed, block[:4_084])
    ):
        raise RefusedError("directory tail checksum or shape mismatch")

    kinds = {1: "regular", 2: "directory", 7: "symlink"}
    entries = []
    offset = 0
    while offset < 4_084:
        inode, record_bytes, name_bytes, file_type = struct.unpack_from(
            "<IHBB", block, offset
        )
        if (
            record_bytes < 8
            or record_bytes % 4
            or offset + record_bytes > 4_084
            or name_bytes > 255
            or name_bytes > record_bytes - 8
        ):
            raise RefusedError("invalid linear directory record")
        raw_name = block[offset + 8 : offset + 8 + name_bytes]
        if inode == 0:
            if name_bytes != 0 or file_type != 0:
                raise RefusedError("unused directory record carries a name or type")
        else:
            if not 1 <= inode <= SEALED_INODES_COUNT or file_type not in kinds:
                raise RefusedError("directory entry points outside the sealed kinds")
            try:
                name = raw_name.decode("utf-8")
            except UnicodeDecodeError as error:
                raise RefusedError("directory name is not UTF-8") from error
            if (
                not name
                or "/" in name
                or "\x00" in name
                or any(ord(character) < 32 or ord(character) == 127 for character in name)
            ):
                raise RefusedError("unsafe directory entry name")
            entries.append(
                DirectoryEntry(
                    inode=inode,
                    name=name,
                    kind=kinds[file_type],
                    record_bytes=record_bytes,
                )
            )
        offset += record_bytes
    if offset != 4_084:
        raise RefusedError("directory records do not meet their checksum tail")
    return entries


def _decode_extent_length(raw: int) -> tuple[int, bool]:
    if raw == 0:
        raise RefusedError("zero-length extent")
    if raw <= 0x8000:
        return raw, False
    return raw - 0x8000, True


def _extent_header(
    node: bytes,
    *,
    exact_bytes: int,
    exact_maximum: int,
    expected_depth: int,
    allow_empty: bool,
) -> int:
    if not isinstance(node, bytes) or len(node) != exact_bytes:
        raise RefusedError("extent node has the wrong context size")
    magic, entries, maximum, depth, generation = struct.unpack_from(
        "<HHHHI", node, 0
    )
    if magic != EXTENT_MAGIC:
        raise RefusedError("bad extent magic")
    if depth != expected_depth:
        raise RefusedError("extent node depth differs from its context")
    if generation != 0:
        raise RefusedError("sealed extent generation must be zero")
    if maximum != exact_maximum or entries > maximum or (not allow_empty and entries == 0):
        raise RefusedError("invalid extent entry count")
    return entries


def _parse_leaf_entries(node: bytes, entries: int) -> List[Extent]:

    result: List[Extent] = []
    logical_end = 0
    physical_ranges = []
    for index in range(entries):
        logical, raw_length, physical_high, physical_low = struct.unpack_from(
            "<IHHI", node, 12 + index * 12
        )
        length, unwritten = _decode_extent_length(raw_length)
        physical = physical_low | (physical_high << 32)
        if (
            physical == 0
            or logical + length > (1 << 32)
            or physical + length > SEALED_BLOCKS_COUNT
            or (result and logical < logical_end)
        ):
            raise RefusedError("overlapping or invalid logical extent")
        logical_end = logical + length
        physical_range = (physical, physical + length)
        if any(
            physical_range[0] < old_end and old_start < physical_range[1]
            for old_start, old_end in physical_ranges
        ):
            raise RefusedError("overlapping physical extent")
        physical_ranges.append(physical_range)
        result.append(
            Extent(
                logical=logical,
                length=length,
                physical=physical,
                unwritten=unwritten,
            )
        )
    return result


def parse_extent_node(node: bytes, expected_depth: int) -> List[Extent]:
    """Parse the exact 60-byte depth-zero extent root stored in an inode."""

    if expected_depth != 0:
        raise RefusedError("this entrypoint accepts only a depth-zero inode root")
    entries = _extent_header(
        node,
        exact_bytes=60,
        exact_maximum=4,
        expected_depth=0,
        allow_empty=True,
    )
    return _parse_leaf_entries(node, entries)


def _parse_extent_indices(node: bytes) -> List[ExtentIndex]:
    entries = _extent_header(
        node,
        exact_bytes=60,
        exact_maximum=4,
        expected_depth=1,
        allow_empty=False,
    )
    result = []
    previous_logical = None
    children = set()
    for index in range(entries):
        logical, child_low, child_high, unused = struct.unpack_from(
            "<IIHH", node, 12 + index * 12
        )
        child = child_low | (child_high << 32)
        if (
            unused != 0
            or child == 0
            or child >= SEALED_BLOCKS_COUNT
            or child in children
            or (previous_logical is not None and logical <= previous_logical)
        ):
            raise RefusedError("invalid depth-one extent index")
        result.append(ExtentIndex(logical=logical, child_physical=child))
        children.add(child)
        previous_logical = logical
    return result


def _parse_external_extent_leaf(node: bytes, inode_checksum_seed: int) -> List[Extent]:
    entries = _extent_header(
        node,
        exact_bytes=4_096,
        exact_maximum=340,
        expected_depth=0,
        allow_empty=False,
    )
    stored_checksum = _u32(node, 4_092)
    if stored_checksum != _crc32c(inode_checksum_seed, node[:4_092]):
        raise RefusedError("external extent leaf checksum mismatch")
    return _parse_leaf_entries(node, entries)


def _refuse_overlapping_extents(extents: List[Extent]) -> None:
    logical_end = 0
    physical_ranges = []
    for index, extent in enumerate(extents):
        if index and extent.logical < logical_end:
            raise RefusedError("overlapping logical extents across leaves")
        logical_end = extent.logical + extent.length
        current = (extent.physical, extent.physical + extent.length)
        if any(
            current[0] < old_end and old_start < current[1]
            for old_start, old_end in physical_ranges
        ):
            raise RefusedError("overlapping physical extents across leaves")
        physical_ranges.append(current)


def parse_extent_tree(
    inode_root: bytes,
    *,
    read_block: Callable[[int], bytes],
    inode_checksum_seed: int,
) -> List[Extent]:
    """Decode the sealed image's supported depth-zero or depth-one tree."""

    if not isinstance(inode_root, bytes) or len(inode_root) != 60:
        raise RefusedError("inode extent root must be exactly 60 bytes")
    depth = _u16(inode_root, 6)
    if depth == 0:
        return parse_extent_node(inode_root, expected_depth=0)
    if depth != 1:
        raise RefusedError("only depth-zero and depth-one extents are supported")
    if not isinstance(inode_checksum_seed, int) or not 0 <= inode_checksum_seed <= 0xFFFFFFFF:
        raise RefusedError("invalid inode checksum seed")

    indices = _parse_extent_indices(inode_root)
    extents = []
    for index in indices:
        try:
            child = read_block(index.child_physical)
        except Exception as error:
            raise RefusedError("failed to read external extent leaf") from error
        rows = _parse_external_extent_leaf(child, inode_checksum_seed)
        if not rows or rows[0].logical != index.logical:
            raise RefusedError("extent index key differs from child first logical block")
        extents.extend(rows)
    _refuse_overlapping_extents(extents)
    return extents
