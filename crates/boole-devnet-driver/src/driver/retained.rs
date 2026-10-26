use super::*;
use boole_devnet_archive::{
    error::{availability, integrity},
    pins::HeaderBinding,
};
use std::collections::BTreeMap;
impl<L: L1Source, E: Engine> Driver<L, E> {
    /// Full replay uses only the retained chain. Only a pre-pin store may
    /// bootstrap historical *local L2* headers, under a fixed durable ceiling.
    pub(super) fn retained_history(
        &mut self,
        parent: &ParentHeader,
    ) -> Result<Option<BTreeMap<u64, HeaderBinding>>> {
        let Some(pins) = &mut self.pins else {
            return Ok(None);
        };
        let source = self.l1.input_journal().context("S20: own input journal")?;
        let source = source.borrow();
        pins.refresh(&source)?;
        if parent.number == 0 {
            let root = State::new(&self.config, self.l2_gas_limit)?.root;
            let b = HeaderBinding {
                header: parent.clone(),
                transactions: vec![],
                root,
            };
            pins.prepare(&source, b.clone(), &[])?;
            pins.select(0, &parent.hash, root)?;
            pins.finish_bootstrap()?;
            return Ok(Some([(0, b)].into()));
        }
        if let Ok(b) = pins.view().binding(parent.number) {
            if b.header.hash != parent.hash {
                return Err(integrity(
                    parent.number,
                    "pin-engine",
                    "Engine head differs from immutable own L2 binding",
                ));
            }
        }
        if self.m2.height() == parent.number {
            pins.view().verify_header(parent.number, parent)?;
            return Ok(Some(BTreeMap::new()));
        }
        match pins.view().chain(parent) {
            Ok(chain) => return Ok(Some(chain)),
            Err(e)
                if !matches!(
                    e.downcast_ref::<boole_devnet_archive::error::Error>(),
                    Some(boole_devnet_archive::error::Error::Availability { .. })
                ) =>
            {
                return Err(e)
            }
            Err(e) if !pins.bootstrap_allowed() => return Err(e),
            Err(_) => {}
        }
        if self
            .checkpoint
            .as_ref()
            .is_some_and(|cp| cp.get("pin_journal").is_some())
        {
            return Err(availability(
                parent.number,
                "pin-bootstrap",
                "S20: modern checkpoint cannot bootstrap missing pins",
            ));
        }
        if source.tip() < parent.number {
            return Err(availability(
                parent.number,
                "pin-bootstrap",
                "S20: full verified own journal required for legacy bootstrap",
            ));
        }
        pins.begin_bootstrap(parent.number)?;
        let mut map = BTreeMap::new();
        for h in 0..=parent.number {
            let header = if h == parent.number {
                parent.clone()
            } else {
                self.engine.header(h)?
            };
            let input = source.get(h)?;
            let root = if h == 0 {
                State::new(&self.config, self.l2_gas_limit)?.root
            } else {
                boole_derivation::system::read_m2_root(
                    header
                        .m2_root_deposit
                        .as_deref()
                        .context("bootstrap root")?,
                    h,
                    &input.block.hash,
                )?
            };
            let b = HeaderBinding {
                header,
                transactions: if h == 0 {
                    vec![]
                } else {
                    self.engine.transactions(h)?
                },
                root,
            };
            b.verify(source.network(), &input.block)?;
            if h == 0 {
                pins.prepare(&source, b.clone(), &[])?;
                pins.select(0, &b.header.hash, root)?;
            }
            map.insert(h, b);
        }
        Ok(Some(map))
    }
}
