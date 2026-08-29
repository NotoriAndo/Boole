"""RED-first tests for the sealed ext4 physical-owner reader."""

from __future__ import annotations

import dataclasses
import importlib
import pathlib
import struct
import sys
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]


def crc32c(seed: int, value: bytes) -> int:
    polynomial = 0x82F63B78
    crc = seed
    for octet in value:
        crc ^= octet
        for _ in range(8):
            crc = (crc >> 1) ^ polynomial if crc & 1 else crc >> 1
    return crc & 0xFFFFFFFF


def module():
    if str(REPO / "scripts") not in sys.path:
        sys.path.insert(0, str(REPO / "scripts"))
    return importlib.import_module("native_shadow_ext4_readonly_owner_map_arm64_v1")


def sealed_superblock() -> bytes:
    block = bytearray(1024)
    struct.pack_into("<I", block, 0x00, 22_528)  # inodes count
    struct.pack_into("<I", block, 0x04, 496_979)  # blocks low
    struct.pack_into("<I", block, 0x14, 0)  # first data block
    struct.pack_into("<I", block, 0x18, 2)  # 1 KiB << 2 = 4 KiB
    struct.pack_into("<I", block, 0x1C, 2)  # no bigalloc: cluster == block
    struct.pack_into("<I", block, 0x20, 32_768)  # blocks/group
    struct.pack_into("<I", block, 0x24, 32_768)  # clusters/group
    struct.pack_into("<I", block, 0x28, 1_408)  # inodes/group
    struct.pack_into("<H", block, 0x38, 0xEF53)
    struct.pack_into("<H", block, 0x3A, 1)  # clean
    struct.pack_into("<I", block, 0x5C, 0x3C)
    struct.pack_into("<I", block, 0x60, 0x2C2)
    struct.pack_into("<I", block, 0x64, 0x46B)
    block[0x68:0x78] = bytes.fromhex("00000000000040008000000000000001")
    struct.pack_into("<H", block, 0x58, 256)  # inode bytes
    struct.pack_into("<I", block, 0x54, 11)  # first non-reserved inode
    struct.pack_into("<H", block, 0xCE, 242)  # reserved GDT blocks
    struct.pack_into("<I", block, 0xE0, 8)  # journal inode
    struct.pack_into("<H", block, 0xFE, 64)  # group descriptor bytes
    block[0x175] = 1  # crc32c
    struct.pack_into("<I", block, 0x150, 0)  # blocks high
    struct.pack_into("<I", block, 0x3FC, crc32c(0xFFFFFFFF, block[:0x3FC]))
    return bytes(block)


def zero_allocation_group_table(mod):
    """A checksum-valid 16-group table whose relevant bitmaps are empty."""

    geometry = mod.parse_superblock(
        sealed_superblock(), image_size=2_035_625_984
    )
    checksum_seed = mod._crc32c(0xFFFFFFFF, geometry.uuid)
    table = bytearray(geometry.group_count * geometry.descriptor_bytes)
    blocks = {}
    for group in range(geometry.group_count):
        offset = group * geometry.descriptor_bytes
        block_bitmap = 2_000 + group
        inode_bitmap = 2_100 + group
        inode_table = 2_200 + group * 88
        valid_blocks = min(
            geometry.blocks_per_group,
            geometry.blocks_count - group * geometry.blocks_per_group,
        )
        descriptor = bytearray(geometry.descriptor_bytes)
        struct.pack_into("<III", descriptor, 0, block_bitmap, inode_bitmap, inode_table)
        struct.pack_into("<HHH", descriptor, 0x0C, valid_blocks, 1_408, 0)
        flags = 0 if group == 0 else 0x3
        struct.pack_into("<H", descriptor, 0x12, flags)
        struct.pack_into("<H", descriptor, 0x1C, 1_408)
        empty_block_bitmap = bytes(4_096)
        empty_inode_bitmap = bytes(176)
        block_checksum = mod._crc32c(checksum_seed, empty_block_bitmap)
        inode_checksum = mod._crc32c(checksum_seed, empty_inode_bitmap)
        if not flags:
            struct.pack_into("<H", descriptor, 0x18, block_checksum & 0xFFFF)
            struct.pack_into("<H", descriptor, 0x1A, inode_checksum & 0xFFFF)
            struct.pack_into("<H", descriptor, 0x38, block_checksum >> 16)
            struct.pack_into("<H", descriptor, 0x3A, inode_checksum >> 16)
        checksum_input = bytearray(descriptor)
        struct.pack_into("<H", checksum_input, 0x1E, 0)
        descriptor_checksum = mod._crc32c(
            mod._crc32c(checksum_seed, struct.pack("<I", group)),
            bytes(checksum_input),
        )
        struct.pack_into("<H", descriptor, 0x1E, descriptor_checksum & 0xFFFF)
        table[offset : offset + geometry.descriptor_bytes] = descriptor
        blocks[block_bitmap] = empty_block_bitmap
        blocks[inode_bitmap] = empty_inode_bitmap + bytes(4_096 - 176)
    return geometry, bytes(table), blocks


class Ext4GeometryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.mod = module()

    def test_the_sealed_geometry_and_feature_masks_are_exact(self) -> None:
        geometry = self.mod.parse_superblock(
            sealed_superblock(),
            image_size=2_035_625_984,
        )
        self.assertEqual(geometry.block_bytes, 4_096)
        self.assertEqual(geometry.blocks_count, 496_979)
        self.assertEqual(geometry.group_count, 16)
        self.assertEqual(geometry.inodes_count, 22_528)
        self.assertEqual(geometry.inodes_per_group, 1_408)
        self.assertEqual(geometry.inode_bytes, 256)
        self.assertEqual(geometry.descriptor_bytes, 64)
        self.assertEqual(
            (geometry.feature_compat, geometry.feature_incompat, geometry.feature_ro_compat),
            (0x3C, 0x2C2, 0x46B),
        )
        self.assertEqual(
            geometry.superblock_checksum,
            crc32c(0xFFFFFFFF, sealed_superblock()[:0x3FC]),
        )

    def test_crc32c_uses_the_ext4_seed_without_a_final_xor(self) -> None:
        self.assertEqual(self.mod._crc32c(0xFFFFFFFF, b"123456789"), 0x1CF96D7C)

    def test_wrong_magic_size_dirty_state_or_feature_drift_is_refused(self) -> None:
        mutations = []
        wrong_magic = bytearray(sealed_superblock())
        struct.pack_into("<H", wrong_magic, 0x38, 0)
        mutations.append((bytes(wrong_magic), 2_035_625_984))
        dirty = bytearray(sealed_superblock())
        struct.pack_into("<H", dirty, 0x3A, 0)
        mutations.append((bytes(dirty), 2_035_625_984))
        feature = bytearray(sealed_superblock())
        struct.pack_into("<I", feature, 0x60, 0x2C2 | 0x8000)
        mutations.append((bytes(feature), 2_035_625_984))
        mutations.append((sealed_superblock(), 2_035_625_983))
        for block, image_size in mutations:
            with self.subTest(image_size=image_size), self.assertRaises(
                self.mod.RefusedError
            ):
                self.mod.parse_superblock(block, image_size=image_size)

    def test_every_sealed_geometry_identity_field_is_exact(self) -> None:
        mutations = []
        for offset, fmt, value in (
            (0x00, "<I", 22_512),
            (0x14, "<I", 1),
            (0x1C, "<I", 3),
            (0x20, "<I", 32_767),
            (0x24, "<I", 32_767),
            (0x28, "<I", 1_407),
        ):
            changed = bytearray(sealed_superblock())
            struct.pack_into(fmt, changed, offset, value)
            mutations.append(bytes(changed))
        uuid = bytearray(sealed_superblock())
        uuid[0x68] ^= 1
        mutations.append(bytes(uuid))
        checksum = bytearray(sealed_superblock())
        checksum[0x3FC] ^= 1
        mutations.append(bytes(checksum))

        for block in mutations:
            with self.subTest(block=block), self.assertRaises(self.mod.RefusedError):
                self.mod.parse_superblock(block, image_size=2_035_625_984)

    def test_rechecks_semantic_superblock_fields_after_a_valid_rechecksum(self) -> None:
        for offset, fmt, value in (
            (0x48, "<I", 1),
            (0x54, "<I", 12),
            (0xCE, "<H", 241),
            (0xE0, "<I", 9),
            (0xE8, "<I", 1),
            (0x270, "<I", 1),
        ):
            changed = bytearray(sealed_superblock())
            struct.pack_into(fmt, changed, offset, value)
            struct.pack_into("<I", changed, 0x3FC, crc32c(0xFFFFFFFF, changed[:0x3FC]))
            with self.subTest(offset=offset), self.assertRaises(self.mod.RefusedError):
                self.mod.parse_superblock(
                    bytes(changed), image_size=2_035_625_984
                )
        checksum_type = bytearray(sealed_superblock())
        checksum_type[0x175] = 0
        struct.pack_into(
            "<I", checksum_type, 0x3FC, crc32c(0xFFFFFFFF, checksum_type[:0x3FC])
        )
        with self.assertRaises(self.mod.RefusedError):
            self.mod.parse_superblock(
                bytes(checksum_type), image_size=2_035_625_984
            )


class ExtentNodeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.mod = module()

    def test_depth_zero_extent_and_length_high_bit_semantics(self) -> None:
        node = bytearray(60)
        struct.pack_into("<HHHHI", node, 0, 0xF30A, 2, 4, 0, 0)
        struct.pack_into("<IHHI", node, 12, 0, 0x8000, 0, 100)
        struct.pack_into("<IHHI", node, 24, 32_768, 0x8001, 0, 32_868)
        extents = self.mod.parse_extent_node(bytes(node), expected_depth=0)
        self.assertEqual(
            [(row.logical, row.length, row.physical, row.unwritten) for row in extents],
            [(0, 32_768, 100, False), (32_768, 1, 32_868, True)],
        )

    def test_an_empty_depth_zero_inode_root_is_a_valid_empty_file(self) -> None:
        node = bytearray(60)
        struct.pack_into("<HHHHI", node, 0, 0xF30A, 0, 4, 0, 0)
        self.assertEqual(
            self.mod.parse_extent_node(bytes(node), expected_depth=0),
            [],
        )

    def test_inode_root_size_capacity_and_extent_ranges_are_exact(self) -> None:
        wrong_size = bytearray(4096)
        struct.pack_into("<HHHHI", wrong_size, 0, 0xF30A, 0, 340, 0, 0)

        logical_overflow = bytearray(60)
        struct.pack_into("<HHHHI", logical_overflow, 0, 0xF30A, 1, 4, 0, 0)
        struct.pack_into("<IHHI", logical_overflow, 12, 0xFFFFFFFF, 2, 0, 100)

        physical_outside = bytearray(60)
        struct.pack_into("<HHHHI", physical_outside, 0, 0xF30A, 1, 4, 0, 0)
        struct.pack_into("<IHHI", physical_outside, 12, 0, 1, 0, 496_979)

        for node in (wrong_size, logical_overflow, physical_outside):
            with self.subTest(size=len(node)), self.assertRaises(self.mod.RefusedError):
                self.mod.parse_extent_node(bytes(node), expected_depth=0)

    def test_extent_header_order_overlap_and_zero_length_are_refused(self) -> None:
        templates = []
        bad_magic = bytearray(36)
        struct.pack_into("<HHHHI", bad_magic, 0, 0, 1, 2, 0, 0)
        struct.pack_into("<IHHI", bad_magic, 12, 0, 1, 0, 100)
        templates.append(bytes(bad_magic))
        zero = bytearray(36)
        struct.pack_into("<HHHHI", zero, 0, 0xF30A, 1, 2, 0, 0)
        struct.pack_into("<IHHI", zero, 12, 0, 0, 0, 100)
        templates.append(bytes(zero))
        overlap = bytearray(48)
        struct.pack_into("<HHHHI", overlap, 0, 0xF30A, 2, 3, 0, 0)
        struct.pack_into("<IHHI", overlap, 12, 0, 2, 0, 100)
        struct.pack_into("<IHHI", overlap, 24, 1, 1, 0, 200)
        templates.append(bytes(overlap))
        generation = bytearray(60)
        struct.pack_into("<HHHHI", generation, 0, 0xF30A, 0, 4, 0, 1)
        templates.append(bytes(generation))
        for node in templates:
            with self.subTest(node=node), self.assertRaises(self.mod.RefusedError):
                self.mod.parse_extent_node(node, expected_depth=0)


class DepthOneExtentTreeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.mod = module()
        self.inode_seed = 0x12345678

    def leaf(self, rows) -> bytes:
        block = bytearray(4096)
        struct.pack_into("<HHHHI", block, 0, 0xF30A, len(rows), 340, 0, 0)
        for index, (logical, length, physical) in enumerate(rows):
            struct.pack_into(
                "<IHHI",
                block,
                12 + index * 12,
                logical,
                length,
                physical >> 32,
                physical & 0xFFFFFFFF,
            )
        struct.pack_into(
            "<I",
            block,
            4092,
            crc32c(self.inode_seed, block[:4092]),
        )
        return bytes(block)

    @staticmethod
    def root(rows) -> bytes:
        node = bytearray(60)
        struct.pack_into("<HHHHI", node, 0, 0xF30A, len(rows), 4, 1, 0)
        for index, (logical, child) in enumerate(rows):
            struct.pack_into(
                "<IIHH",
                node,
                12 + index * 12,
                logical,
                child & 0xFFFFFFFF,
                child >> 32,
                0,
            )
        return bytes(node)

    def test_depth_one_leaf_is_checksum_bound_and_flattened(self) -> None:
        blocks = {1_000: self.leaf([(0, 1, 2_000), (1, 2, 3_000)])}
        extents = self.mod.parse_extent_tree(
            self.root([(0, 1_000)]),
            read_block=blocks.__getitem__,
            inode_checksum_seed=self.inode_seed,
        )
        self.assertEqual(
            [(row.logical, row.length, row.physical) for row in extents],
            [(0, 1, 2_000), (1, 2, 3_000)],
        )

        changed = bytearray(blocks[1_000])
        changed[20] ^= 1
        with self.assertRaises(self.mod.RefusedError):
            self.mod.parse_extent_tree(
                self.root([(0, 1_000)]),
                read_block=lambda _block: bytes(changed),
                inode_checksum_seed=self.inode_seed,
            )

    def test_depth_one_index_and_child_boundaries_fail_closed(self) -> None:
        valid_leaf = self.leaf([(0, 1, 2_000)])
        cases = [
            self.root([(0, 0)]),
            self.root([(0, 496_979)]),
            self.root([(1, 1_000)]),
            self.root([(0, 1_000), (0, 1_001)]),
        ]
        for root in cases:
            with self.subTest(root=root), self.assertRaises(self.mod.RefusedError):
                self.mod.parse_extent_tree(
                    root,
                    read_block=lambda _block: valid_leaf,
                    inode_checksum_seed=self.inode_seed,
                )

    def test_logical_and_physical_overlap_across_leaves_is_refused(self) -> None:
        blocks = {
            1_000: self.leaf([(0, 2, 2_000)]),
            1_001: self.leaf([(2, 1, 2_001)]),
        }
        with self.assertRaises(self.mod.RefusedError):
            self.mod.parse_extent_tree(
                self.root([(0, 1_000), (2, 1_001)]),
                read_block=blocks.__getitem__,
                inode_checksum_seed=self.inode_seed,
            )


class AllocationMetadataTests(unittest.TestCase):
    def setUp(self) -> None:
        self.mod = module()
        self.geometry, self.table, self.blocks = zero_allocation_group_table(
            self.mod
        )

    def test_group_descriptors_and_both_bitmaps_are_checksum_bound(self) -> None:
        descriptors = self.mod.parse_group_descriptors(
            self.table, geometry=self.geometry
        )
        allocated = self.mod.verify_allocation_bitmaps(
            descriptors,
            geometry=self.geometry,
            read_block=self.blocks.__getitem__,
        )
        self.assertEqual(len(descriptors), 16)
        self.assertEqual(
            sum(len(rows) for rows in allocated.inodes_by_group.values()), 0
        )
        self.assertEqual(allocated.blocks, frozenset())
        self.assertEqual(descriptors[0].inode_table, 2_200)

        changed_descriptor = bytearray(self.table)
        changed_descriptor[0x0C] ^= 1
        with self.assertRaises(self.mod.RefusedError):
            self.mod.parse_group_descriptors(
                bytes(changed_descriptor), geometry=self.geometry
            )

        changed_blocks = dict(self.blocks)
        bad_bitmap = bytearray(changed_blocks[2_000])
        bad_bitmap[0] ^= 1
        changed_blocks[2_000] = bytes(bad_bitmap)
        with self.assertRaises(self.mod.RefusedError):
            self.mod.verify_allocation_bitmaps(
                descriptors,
                geometry=self.geometry,
                read_block=changed_blocks.__getitem__,
            )

    def test_allocation_counts_and_uninitialised_flags_cannot_disagree(self) -> None:
        table = bytearray(self.table)
        descriptor = bytearray(table[:64])
        struct.pack_into("<H", descriptor, 0x12, 0)
        struct.pack_into("<H", descriptor, 0x0E, 1_407)
        struct.pack_into("<H", descriptor, 0x1C, 1_407)
        checksum_seed = self.mod._crc32c(0xFFFFFFFF, self.geometry.uuid)
        struct.pack_into("<H", descriptor, 0x1E, 0)
        checksum = self.mod._crc32c(
            self.mod._crc32c(checksum_seed, struct.pack("<I", 0)),
            bytes(descriptor),
        )
        struct.pack_into("<H", descriptor, 0x1E, checksum & 0xFFFF)
        table[:64] = descriptor
        descriptors = self.mod.parse_group_descriptors(
            bytes(table), geometry=self.geometry
        )
        with self.assertRaises(self.mod.RefusedError):
            self.mod.verify_allocation_bitmaps(
                descriptors,
                geometry=self.geometry,
                read_block=self.blocks.__getitem__,
            )

    def test_allocated_inode_reader_uses_the_group_table_location(self) -> None:
        descriptors = self.mod.parse_group_descriptors(
            self.table, geometry=self.geometry
        )
        helper = InodeTests("test_inode_checksum_identity_and_extent_root_are_bound")
        helper.setUp()
        raw = helper.inode(42)
        inode_index = 41
        table_block = descriptors[0].inode_table + (inode_index * 256) // 4_096
        offset = (inode_index * 256) % 4_096
        block = bytearray(4_096)
        block[offset : offset + 256] = raw
        blocks = dict(self.blocks)
        blocks[table_block] = bytes(block)
        allocation = self.mod.AllocationMap(
            inodes_by_group={
                group: ([42] if group == 0 else []) for group in range(16)
            },
            blocks=frozenset({table_block}),
        )
        inodes = self.mod.read_allocated_inodes(
            descriptors,
            allocation=allocation,
            geometry=self.geometry,
            read_block=blocks.__getitem__,
        )
        self.assertEqual(list(inodes), [42])
        self.assertEqual(inodes[42].kind, "regular")


class InodeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.mod = module()
        self.geometry = self.mod.parse_superblock(
            sealed_superblock(), image_size=2_035_625_984
        )

    def inode(self, number: int, *, mode: int = 0o100444, size: int = 4_096) -> bytes:
        raw = bytearray(256)
        struct.pack_into("<H", raw, 0x00, mode)
        struct.pack_into("<I", raw, 0x04, size & 0xFFFFFFFF)
        if mode:
            struct.pack_into("<H", raw, 0x1A, 1)
            struct.pack_into("<I", raw, 0x20, 0x80000)
            struct.pack_into("<HHHHI", raw, 0x28, 0xF30A, 1, 4, 0, 0)
            struct.pack_into("<IHHI", raw, 0x34, 0, 1, 0, 4_000)
        struct.pack_into("<I", raw, 0x64, 7)
        struct.pack_into("<I", raw, 0x6C, size >> 32)
        struct.pack_into("<H", raw, 0x80, 32)
        seed = self.mod.inode_checksum_seed(
            self.geometry, inode_number=number, generation=7
        )
        checksum_input = bytearray(raw)
        struct.pack_into("<H", checksum_input, 0x7C, 0)
        struct.pack_into("<H", checksum_input, 0x82, 0)
        checksum = self.mod._crc32c(seed, bytes(checksum_input))
        struct.pack_into("<H", raw, 0x7C, checksum & 0xFFFF)
        struct.pack_into("<H", raw, 0x82, checksum >> 16)
        return bytes(raw)

    def test_inode_checksum_identity_and_extent_root_are_bound(self) -> None:
        parsed = self.mod.parse_inode(
            self.inode(42), inode_number=42, geometry=self.geometry
        )
        self.assertEqual(parsed.number, 42)
        self.assertEqual(parsed.kind, "regular")
        self.assertEqual(parsed.size, 4_096)
        self.assertEqual(parsed.generation, 7)
        self.assertEqual(parsed.extents[0].physical, 4_000)

        changed = bytearray(self.inode(42))
        changed[0x20] ^= 1
        with self.assertRaises(self.mod.RefusedError):
            self.mod.parse_inode(
                bytes(changed), inode_number=42, geometry=self.geometry
            )
        with self.assertRaises(self.mod.RefusedError):
            self.mod.parse_inode(
                self.inode(42), inode_number=43, geometry=self.geometry
            )

    def test_old_inode_layout_uses_only_the_low_checksum_half(self) -> None:
        raw = bytearray(self.inode(1, mode=0, size=0))
        struct.pack_into("<H", raw, 0x80, 0)
        struct.pack_into("<H", raw, 0x82, 0)
        struct.pack_into("<H", raw, 0x7C, 0)
        seed = self.mod.inode_checksum_seed(
            self.geometry, inode_number=1, generation=7
        )
        checksum = self.mod._crc32c(seed, bytes(raw))
        struct.pack_into("<H", raw, 0x7C, checksum & 0xFFFF)
        parsed = self.mod.parse_inode(
            bytes(raw), inode_number=1, geometry=self.geometry
        )
        self.assertEqual(parsed.kind, "reserved")
        self.assertEqual(parsed.checksum_bits, 16)

    def test_inline_xattr_tail_must_be_empty_even_with_a_valid_inode_checksum(self) -> None:
        raw = bytearray(self.inode(42))
        raw[0xA0] = 1
        struct.pack_into("<H", raw, 0x7C, 0)
        struct.pack_into("<H", raw, 0x82, 0)
        seed = self.mod.inode_checksum_seed(
            self.geometry, inode_number=42, generation=7
        )
        checksum = self.mod._crc32c(seed, bytes(raw))
        struct.pack_into("<H", raw, 0x7C, checksum & 0xFFFF)
        struct.pack_into("<H", raw, 0x82, checksum >> 16)
        with self.assertRaises(self.mod.RefusedError):
            self.mod.parse_inode(
                bytes(raw), inode_number=42, geometry=self.geometry
            )

    def test_depth_one_inode_reads_a_checksum_bound_external_leaf(self) -> None:
        raw = bytearray(self.inode(42))
        root = bytearray(60)
        struct.pack_into("<HHHHI", root, 0, 0xF30A, 1, 4, 1, 0)
        struct.pack_into("<IIHH", root, 12, 0, 3_500, 0, 0)
        raw[0x28:0x64] = root
        struct.pack_into("<H", raw, 0x7C, 0)
        struct.pack_into("<H", raw, 0x82, 0)
        seed = self.mod.inode_checksum_seed(
            self.geometry, inode_number=42, generation=7
        )
        checksum = self.mod._crc32c(seed, bytes(raw))
        struct.pack_into("<H", raw, 0x7C, checksum & 0xFFFF)
        struct.pack_into("<H", raw, 0x82, checksum >> 16)

        leaf = bytearray(4_096)
        struct.pack_into("<HHHHI", leaf, 0, 0xF30A, 1, 340, 0, 0)
        struct.pack_into("<IHHI", leaf, 12, 0, 1, 0, 4_000)
        struct.pack_into(
            "<I", leaf, 4_092, self.mod._crc32c(seed, leaf[:4_092])
        )
        parsed = self.mod.parse_inode(
            bytes(raw),
            inode_number=42,
            geometry=self.geometry,
            read_block=lambda block: bytes(leaf) if block == 3_500 else None,
        )
        self.assertEqual(parsed.extents[0].physical, 4_000)
        self.assertEqual(parsed.extent_tree_blocks, (3_500,))

    def test_only_the_exact_fast_symlink_shape_may_omit_extents(self) -> None:
        raw = bytearray(self.inode(42, mode=0o120777, size=3))
        struct.pack_into("<I", raw, 0x1C, 0)
        struct.pack_into("<I", raw, 0x20, 0)
        raw[0x28:0x64] = b"lib" + bytes(57)
        struct.pack_into("<H", raw, 0x7C, 0)
        struct.pack_into("<H", raw, 0x82, 0)
        seed = self.mod.inode_checksum_seed(
            self.geometry, inode_number=42, generation=7
        )
        checksum = self.mod._crc32c(seed, bytes(raw))
        struct.pack_into("<H", raw, 0x7C, checksum & 0xFFFF)
        struct.pack_into("<H", raw, 0x82, checksum >> 16)
        parsed = self.mod.parse_inode(
            bytes(raw), inode_number=42, geometry=self.geometry
        )
        self.assertEqual(parsed.kind, "symlink")
        self.assertEqual(parsed.fast_symlink_target, b"lib")

        wrong = bytearray(raw)
        struct.pack_into("<I", wrong, 0x1C, 1)
        struct.pack_into("<H", wrong, 0x7C, 0)
        struct.pack_into("<H", wrong, 0x82, 0)
        checksum = self.mod._crc32c(seed, bytes(wrong))
        struct.pack_into("<H", wrong, 0x7C, checksum & 0xFFFF)
        struct.pack_into("<H", wrong, 0x82, checksum >> 16)
        with self.assertRaises(self.mod.RefusedError):
            self.mod.parse_inode(
                bytes(wrong), inode_number=42, geometry=self.geometry
            )

    def test_inode_payload_reader_binds_size_blocks_and_fast_symlinks(self) -> None:
        regular = self.mod.parse_inode(
            self.inode(42, size=4_097),
            inode_number=42,
            geometry=self.geometry,
        )
        self.assertEqual(
            self.mod.read_inode_payload(regular, lambda _block: b"x" * 4_096),
            b"x" * 4_096 + b"\0",
        )

        exact = self.mod.parse_inode(
            self.inode(42, size=4_096),
            inode_number=42,
            geometry=self.geometry,
        )
        self.assertEqual(
            self.mod.read_inode_payload(
                exact,
                lambda block: bytes([block % 251]) * 4_096,
            ),
            bytes([4_000 % 251]) * 4_096,
        )

        raw = bytearray(self.inode(43, mode=0o120777, size=3))
        struct.pack_into("<I", raw, 0x1C, 0)
        struct.pack_into("<I", raw, 0x20, 0)
        raw[0x28:0x64] = b"lib" + bytes(57)
        struct.pack_into("<H", raw, 0x7C, 0)
        struct.pack_into("<H", raw, 0x82, 0)
        seed = self.mod.inode_checksum_seed(
            self.geometry, inode_number=43, generation=7
        )
        checksum = self.mod._crc32c(seed, bytes(raw))
        struct.pack_into("<H", raw, 0x7C, checksum & 0xFFFF)
        struct.pack_into("<H", raw, 0x82, checksum >> 16)
        symlink = self.mod.parse_inode(
            bytes(raw), inode_number=43, geometry=self.geometry
        )
        self.assertEqual(
            self.mod.read_inode_payload(
                symlink,
                lambda _block: self.fail("fast symlink must not read a data block"),
            ),
            b"lib",
        )

        # Allocated data outside ceil(i_size / block_size) is not a sparse hole;
        # it is an extent that claims bytes beyond the logical file and fails.
        too_short = dataclasses.replace(exact, size=0)
        with self.assertRaises(self.mod.RefusedError):
            self.mod.read_inode_payload(too_short, lambda _block: b"x" * 4_096)


class DirectoryBlockTests(unittest.TestCase):
    def setUp(self) -> None:
        self.mod = module()
        helper = InodeTests("test_inode_checksum_identity_and_extent_root_are_bound")
        helper.setUp()
        self.directory = self.mod.parse_inode(
            helper.inode(42, mode=0o040755),
            inode_number=42,
            geometry=helper.geometry,
        )

    def block(self) -> bytes:
        block = bytearray(4_096)
        struct.pack_into("<IHBB", block, 0, 42, 12, 1, 2)
        block[8:9] = b"."
        struct.pack_into("<IHBB", block, 12, 2, 12, 2, 2)
        block[20:22] = b".."
        struct.pack_into("<IHBB", block, 24, 100, 4_060, 4, 1)
        block[32:36] = b"file"
        struct.pack_into("<IHBB", block, 4_084, 0, 12, 0, 0xDE)
        struct.pack_into(
            "<I",
            block,
            4_092,
            self.mod._crc32c(self.directory.checksum_seed, block[:4_084]),
        )
        return bytes(block)

    def test_linear_directory_entries_and_tail_are_checksum_bound(self) -> None:
        entries = self.mod.parse_directory_block(
            self.block(), directory_inode=self.directory
        )
        self.assertEqual(
            [(row.name, row.inode, row.kind) for row in entries],
            [(".", 42, "directory"), ("..", 2, "directory"), ("file", 100, "regular")],
        )

        changed = bytearray(self.block())
        changed[32] ^= 1
        with self.assertRaises(self.mod.RefusedError):
            self.mod.parse_directory_block(
                bytes(changed), directory_inode=self.directory
            )

    def test_directory_tail_record_lengths_and_names_fail_closed(self) -> None:
        cases = []
        bad_tail = bytearray(self.block())
        bad_tail[4_091] = 0
        cases.append(bad_tail)
        bad_length = bytearray(self.block())
        struct.pack_into("<H", bad_length, 28, 4_058)
        cases.append(bad_length)
        bad_name = bytearray(self.block())
        bad_name[32:36] = b"a/b\x00"
        cases.append(bad_name)
        for block in cases:
            with self.subTest(), self.assertRaises(self.mod.RefusedError):
                self.mod.parse_directory_block(
                    bytes(block), directory_inode=self.directory
                )


class ResizeMetadataTests(unittest.TestCase):
    def setUp(self) -> None:
        self.mod = module()
        self.resize_inode = self.mod.Inode(
            number=7,
            kind="resize-metadata",
            mode=0o100600,
            uid=0,
            gid=0,
            size=4_299_210_752,
            links=1,
            flags=0,
            generation=0,
            checksum=0,
            checksum_bits=32,
            checksum_seed=0,
            extents=(),
            raw_block_map=struct.pack("<15I", *((0,) * 13 + (1_689, 0))),
            blocks_512=11_624,
            fast_symlink_target=None,
            extent_tree_blocks=(),
        )
        double_indirect = bytearray(4_096)
        struct.pack_into("<242I", double_indirect, 4, *range(2, 244))
        self.blocks = {1_689: bytes(double_indirect)}
        for block in range(2, 244):
            indirect = bytearray(4_096)
            pointers = tuple(
                group * 32_768 + block for group in (1, 3, 5, 7, 9)
            )
            struct.pack_into("<5I", indirect, 0, *pointers)
            self.blocks[block] = bytes(indirect)
        self.expected = frozenset(
            {1_689, *range(2, 244)}
            | {
                group * 32_768 + block
                for group in (1, 3, 5, 7, 9)
                for block in range(2, 244)
            }
        )

    def test_resize_inode_indirection_is_exact_and_fully_allocated(self) -> None:
        observed = self.mod.read_resize_metadata_blocks(
            self.resize_inode,
            read_block=self.blocks.__getitem__,
            allocated_blocks=self.expected,
        )
        self.assertEqual(observed, self.expected)
        self.assertEqual(len(observed), 1_453)

    def test_resize_pointer_drift_or_free_block_is_refused(self) -> None:
        changed = dict(self.blocks)
        indirect = bytearray(changed[2])
        struct.pack_into("<I", indirect, 0, 32_771)
        changed[2] = bytes(indirect)
        with self.assertRaises(self.mod.RefusedError):
            self.mod.read_resize_metadata_blocks(
                self.resize_inode,
                read_block=changed.__getitem__,
                allocated_blocks=self.expected,
            )
        with self.assertRaises(self.mod.RefusedError):
            self.mod.read_resize_metadata_blocks(
                self.resize_inode,
                read_block=self.blocks.__getitem__,
                allocated_blocks=self.expected - {32_770},
            )


class GlobalBlockOwnershipTests(unittest.TestCase):
    def test_every_allocated_block_has_exactly_one_typed_owner(self) -> None:
        mod = module()
        geometry, table, bitmap_blocks = zero_allocation_group_table(mod)
        descriptors = mod.parse_group_descriptors(table, geometry=geometry)
        resize = ResizeMetadataTests("test_resize_inode_indirection_is_exact_and_fully_allocated")
        resize.setUp()
        inode_helper = InodeTests("test_inode_checksum_identity_and_extent_root_are_bound")
        inode_helper.setUp()
        regular = mod.parse_inode(
            inode_helper.inode(42), inode_number=42, geometry=geometry
        )
        metadata = set()
        for group in (0, 1, 3, 5, 7, 9):
            metadata.update(
                {
                    group * geometry.blocks_per_group,
                    group * geometry.blocks_per_group + 1,
                }
            )
        for descriptor in descriptors:
            metadata.update({descriptor.block_bitmap, descriptor.inode_bitmap})
            metadata.update(range(descriptor.inode_table, descriptor.inode_table + 88))
        allocated = frozenset(metadata | set(resize.expected) | {4_000})
        blocks = dict(bitmap_blocks)
        blocks.update(resize.blocks)
        owners = mod.build_block_ownership(
            geometry=geometry,
            descriptors=descriptors,
            allocation=mod.AllocationMap(
                inodes_by_group={group: [] for group in range(16)},
                blocks=allocated,
            ),
            inodes={7: resize.resize_inode, 42: regular},
            read_block=blocks.__getitem__,
            expected_counts={
                "super-gdt": 12,
                "allocation-metadata": 1_440,
                "resize-metadata": 1_453,
                "extent-metadata": 0,
                "file-data": 1,
                "directory-data": 0,
                "symlink-data": 0,
                "journal": 0,
            },
        )
        self.assertEqual(set(owners), set(allocated))
        self.assertEqual(owners[4_000].inode, 42)
        self.assertEqual(owners[4_000].logical, 0)

        with self.assertRaises(mod.RefusedError):
            mod.build_block_ownership(
                geometry=geometry,
                descriptors=descriptors,
                allocation=mod.AllocationMap(
                    inodes_by_group={group: [] for group in range(16)},
                    blocks=allocated - {4_000},
                ),
                inodes={7: resize.resize_inode, 42: regular},
                read_block=blocks.__getitem__,
                expected_counts={
                    "super-gdt": 12,
                    "allocation-metadata": 1_440,
                    "resize-metadata": 1_453,
                    "extent-metadata": 0,
                    "file-data": 1,
                    "directory-data": 0,
                    "symlink-data": 0,
                    "journal": 0,
                },
            )


class DirectoryTreeTests(unittest.TestCase):
    def inode(self, number, kind, mode, size, links, physical, seed):
        return module().Inode(
            number=number,
            kind=kind,
            mode=mode,
            uid=0,
            gid=0,
            size=size,
            links=links,
            flags=0x80000,
            generation=0,
            checksum=0,
            checksum_bits=32,
            checksum_seed=seed,
            extents=(module().Extent(0, 1, physical, False),),
            raw_block_map=bytes(60),
            blocks_512=8,
            fast_symlink_target=None,
            extent_tree_blocks=(),
        )

    @staticmethod
    def directory_block(inode, parent, children, seed):
        rows = [(inode, ".", 2), (parent, "..", 2), *children]
        block = bytearray(4_096)
        offset = 0
        for index, (target, name, kind) in enumerate(rows):
            encoded = name.encode("utf-8")
            minimum = (8 + len(encoded) + 3) & ~3
            record_bytes = 4_084 - offset if index == len(rows) - 1 else minimum
            struct.pack_into(
                "<IHBB", block, offset, target, record_bytes, len(encoded), kind
            )
            block[offset + 8 : offset + 8 + len(encoded)] = encoded
            offset += record_bytes
        struct.pack_into("<IHBB", block, 4_084, 0, 12, 0, 0xDE)
        struct.pack_into("<I", block, 4_092, crc32c(seed, block[:4_084]))
        return bytes(block)

    def test_root_walk_binds_dot_dotdot_types_links_and_unique_paths(self) -> None:
        mod = module()
        inodes = {
            2: self.inode(2, "directory", 0o040755, 4_096, 3, 100, 0x22),
            3: self.inode(3, "regular", 0o100444, 4, 1, 101, 0x33),
            4: self.inode(4, "directory", 0o040755, 4_096, 2, 102, 0x44),
        }
        blocks = {
            100: self.directory_block(2, 2, [(3, "file", 1), (4, "sub", 2)], 0x22),
            102: self.directory_block(4, 2, [], 0x44),
        }
        owners = {
            100: mod.BlockOwner("directory-data", 2, 0),
            101: mod.BlockOwner("file-data", 3, 0),
            102: mod.BlockOwner("directory-data", 4, 0),
        }
        graph = mod.walk_directory_tree(
            inodes=inodes,
            owners=owners,
            read_block=blocks.__getitem__,
        )
        self.assertEqual(graph.path_to_inode, {"file": 3, "sub": 4})
        self.assertEqual(graph.paths_by_inode, {3: ("file",), 4: ("sub",)})

        bad_sub = dict(blocks)
        bad_sub[102] = self.directory_block(4, 99, [], 0x44)
        with self.assertRaises(mod.RefusedError):
            mod.walk_directory_tree(
                inodes=inodes,
                owners=owners,
                read_block=bad_sub.__getitem__,
            )


class RawHitOwnerTests(unittest.TestCase):
    def test_full_marker_span_maps_to_one_regular_inode_and_logical_offset(self) -> None:
        mod = module()
        inode = mod.Inode(
            number=3,
            kind="regular",
            mode=0o100444,
            uid=0,
            gid=0,
            size=7 * 4_096,
            links=1,
            flags=0x80000,
            generation=0,
            checksum=0,
            checksum_bits=32,
            checksum_seed=0,
            extents=(mod.Extent(5, 2, 100, False),),
            raw_block_map=bytes(60),
            blocks_512=16,
            fast_symlink_target=None,
            extent_tree_blocks=(),
        )
        needle = b"marker-crosses"
        first = bytearray(4_096)
        second = bytearray(4_096)
        first[4_090:] = needle[:6]
        second[: len(needle) - 6] = needle[6:]
        owners = {
            100: mod.BlockOwner("file-data", 3, 5),
            101: mod.BlockOwner("file-data", 3, 6),
        }
        mapped = mod.map_raw_hit(
            marker="example",
            raw_offset=100 * 4_096 + 4_090,
            needle=needle,
            image_size=2_035_625_984,
            owners=owners,
            inodes={3: inode},
            paths_by_inode={3: ("usr/share/example",)},
            read_block={100: bytes(first), 101: bytes(second)}.__getitem__,
        )
        self.assertEqual(mapped.inode, 3)
        self.assertEqual(mapped.file_offset, 5 * 4_096 + 4_090)
        self.assertEqual(mapped.paths, ("usr/share/example",))
        self.assertEqual(mapped.physical_blocks, (100, 101))

        ambiguous = dict(owners)
        ambiguous[101] = mod.BlockOwner("file-data", 4, 0)
        with self.assertRaises(mod.RefusedError):
            mod.map_raw_hit(
                marker="example",
                raw_offset=100 * 4_096 + 4_090,
                needle=needle,
                image_size=2_035_625_984,
                owners=ambiguous,
                inodes={3: inode},
                paths_by_inode={3: ("usr/share/example",)},
                read_block={100: bytes(first), 101: bytes(second)}.__getitem__,
            )


if __name__ == "__main__":
    unittest.main()
