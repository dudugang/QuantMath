use risk::Bumpable;
use dates::Date;
use dates::datetime::DateTime;
use core::qm;
use std::rc::Rc;
use std::collections::HashMap;
use instruments::Instrument;
use instruments::fix_all;
use instruments::PricingContext;
use data::fixings::FixingTable;
use data::bumpspotdate::BumpSpotDate;
use data::bumpspotdate::SpotDynamics;
use data::bump::Bump;
use risk::dependencies::DependencyCollector;

/// Bump that defines all the supported bumps to the spot date and ex-from
/// date. This bump has to live in risk rather than data, because it affects
/// all market data, not just one curve at a time.
pub struct BumpTime {
    spot_date_bump: BumpSpotDate,
    _ex_from: Date
}

impl BumpTime {
    pub fn new(spot_date: Date, ex_from: Date, spot_dynamics: SpotDynamics) -> BumpTime {
        BumpTime { spot_date_bump: BumpSpotDate::new(spot_date, spot_dynamics),
            _ex_from: ex_from }
    }

    pub fn apply(&self, instruments: &mut Vec<(f64, Rc<Instrument>)>,
        bumpable: &mut Bumpable) -> Result<(), qm::Error> {

        // Modify the vector of instruments, if any fixings between the old and new spot dates
        // affect any of them.
        self.update_instruments(instruments, bumpable.context(), bumpable.dependencies()?)?;
        
        // Now apply a bump to the model, to shift the spot date. We create a saveable area
        // just to simplify the code. It is not used to actually save anything.
        let mut saveable = bumpable.new_saveable();
        let bump = Bump::new_spot_date(self.spot_date_bump.clone());
        bumpable.bump(&bump, &mut *saveable)?;
        Ok(())
    }

    pub fn update_instruments(&self, instruments: &mut Vec<(f64, Rc<Instrument>)>,
        context: &PricingContext, dependencies: &DependencyCollector) -> Result<(), qm::Error> {

        // are there any fixings between the old and new spot dates?
        let old_spot_date = context.spot_date();
        let new_spot_date = self.spot_date_bump.spot_date();

        // Create a fixing table with any fixings between the old and
        // new spot dates. Note that we do not have to bother with existing
        // fixings, as these have already been entirely taken into account
        // by the list of instruments.
        let mut fixing_map = HashMap::new();
        for (id, instrument) in dependencies.instruments_iter() {
            for fixing in dependencies.fixings(id).iter() {
                let date = fixing.date();
                if date >= old_spot_date && date < new_spot_date {
                    let value = match self.spot_date_bump.spot_dynamics() {
                        SpotDynamics::StickyForward => {
                            // it looks inefficient to keep fetching the curves each time round
                            // the loop, but by far the most common case has at most one fixing
                            let inst: &Instrument = &*instrument.clone();
                            let curve = context.forward_curve(inst, new_spot_date)?;
                            curve.forward(date)? },
                        SpotDynamics::StickySpot => {
                            context.spot(id)? }
                    };

                    fixing_map.entry(id.to_string()).or_insert(Vec::<(DateTime, f64)>::new())
                        .push((*fixing, value));
                }           
            }
        }

        // Apply the fixings to each of the instruments, and build up a new vector of them
        if !fixing_map.is_empty() {
            let fixing_table = FixingTable::from_map(new_spot_date, &fixing_map)?;
            let mut replacement = fix_all(instruments, &fixing_table)?;
            instruments.clear();
            instruments.append(&mut replacement);
        }

        Ok(())
    }
}
