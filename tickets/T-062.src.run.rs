// EXCERPT of server/crates/server/src/run.rs (current file). Line ranges are marked.
// ---- lines 540-700: Run::resolve and Run::start_level ----
        }
        if enemies != offer.enemy_picks || rewards != offer.reward_picks {
            return Err("wrong number of picks");
        }
        Ok(())
    }

    /// Tally everyone's (validated) votes, apply the winning cards and start the generated level.
    /// Ties go to the card offered first. The number of voters is the player count used for gift generation.
    pub fn resolve(&mut self, content: &Content, votes: &[Vec<u8>]) {
        let Some(offer) = self.offer.take() else { return };
        let mut tally: BTreeMap<usize, u32> = BTreeMap::new();
        for v in votes {
            for &p in v {
                *tally.entry(p as usize).or_default() += 1;
            }
        }
        let mut ranked: Vec<usize> = (0..offer.cards.len()).collect();
        ranked.sort_by_key(|i| (std::cmp::Reverse(tally.get(i).copied().unwrap_or(0)), *i));
        let winners = |enemy: bool, n: u8| -> Vec<usize> {
            ranked
                .iter()
                .copied()
                .filter(|&i| (offer.cards[i].kind == CARD_ENEMY) == enemy)
                .take(n as usize)
                .collect()
        };
        let enemy_winners = winners(true, offer.enemy_picks);
        self.most_voted_enemy = enemy_winners.first().map(|&i| offer.cards[i].id.clone());
        for i in enemy_winners {
            self.drafted_enemies.push(offer.cards[i].id.clone());
        }
        for i in winners(false, offer.reward_picks) {
            let card = &offer.cards[i];
            let def = if card.kind == CARD_UPGRADE {
                content.upgrade(&card.id)
            } else {
                content.curse(&card.id)
            };
            if let Some(def) = def {
                let lives_before = self.mods.extra_lives;
                self.mods.apply(&def.mods);
                self.lives = self
                    .lives
                    .saturating_add(self.mods.extra_lives.saturating_sub(lives_before));
                if card.kind == CARD_UPGRADE {
                    self.upgrades.push(card.id.clone());
                } else {
                    self.curses.push(card.id.clone());
                }
            }
        }
        let players = u32::try_from(votes.len()).unwrap_or(1).max(1);
        self.start_level(content, players);
    }

    fn start_level(&mut self, content: &Content, players: u32) {
        let seed = self.rng.next_u32();
        let generated = levelgen::generate_for(seed, self.level_num, players);
        let spawn_slots = generated.enemy_spawns.len();
        self.level = generated.data;
        self.world = World::from_level(&self.level);
        self.gifts_taken.clear();
        self.golden_taken.clear();
        self.enemies.clear();
        self.hazards.reset();
        self.altars_used.clear();
        self.level_phase = LEVEL_COLLECT;
        self.collapse_elapsed_ticks = 0;
        self.collapse = build_collapse_plan(
            &self.level.platforms,
            &self.level.beacon,
            &self.level.spawn,
            self.level_num,
        );
        self.gift_platform = gift_platform_map(&self.level.platforms, &self.level.gifts);
        // The shop lives in the lobby only; starting a level closes it.
        self.shop = None;
        self.event_friction_mul = event_friction_mul(content, self.level.event);
        // Roster: drafted enemies + curse copies + escalation copies (design §5, from level 4 on).
        let mut roster: Vec<String> = self.drafted_enemies.clone();
        let drafted = self.drafted_enemies.len();
        if drafted > 0 {
            for k in 0..self.mods.extra_enemies as usize {
                roster.push(self.drafted_enemies[k % drafted].clone());
            }
        }
        roster.extend(escalation_extra_copies(
            self.level_num,
            &self.drafted_enemies,
            self.most_voted_enemy.as_deref(),
        ));
        // Level 1 stays plain (design A4): no enemies at all.
        if self.level_num == 1 {
            roster.clear();
        }
        // The hub has 8 fixed spawn slots; extra copies cycle onto the slots.
        for (index, id) in roster.iter().enumerate() {
            let Some((content_index, def)) = content.enemy(id) else {
                continue;
            };
            let mut params = self.mods.enemy_params(&def.params);
            params.speed_mul *= escalation_speed_mul(self.level_num);
            let mut pos = if spawn_slots == 0 {
                self.level.spawn
            } else {
                generated.enemy_spawns[index % spawn_slots]
            };
            // Mimics pose as gifts: spawn them on the platform below the generated spawn
            // (pos.y = platform top + 0.5). If the spawn is not above any platform, keep the position.
            if params.behavior == BEHAVIOR_MIMIC
                && let Some(top) = platform_top_below(&self.level.platforms, pos.x, pos.z, pos.y)
            {
                pos.y = top + 0.5;
            } else if let Some(top) = platform_top_below(&self.level.platforms, pos.x, pos.z, pos.y + 0.01) {
                // Everything else starts with its body clear of the floor and its centre at least at a
                // player's chest height: hub enemy slots lie ON the floor (y = top), and a turret whose
                // centre sits on the floor surface had its line of sight and its flat bolts start inside
                // the floor, so it never fired.
                pos.y = pos.y.max(top + params.size.max(0.9) + 0.1);
            }
            let enemy_id = u32::try_from(index).unwrap_or(u32::MAX - 1) + 1;
            // kind = index into content.json enemies, so the client draws the right model.
            self.enemies.push(rules::spawn_enemy_with(
                enemy_id,
                u8::try_from(content_index).unwrap_or(u8::MAX),
                pos,
                params,
            ));
        }
        self.phase = PHASE_PLAYING;
    }

    /// The beacon was reached: back to the lobby to draft for the next level.
    pub fn finish_level(&mut self, content: &Content) {
        self.level_num = self.level_num.saturating_add(1);
        self.level = levelgen::hub();
        self.world = World::from_level(&self.level);
        self.gifts_taken.clear();
        self.golden_taken.clear();
        self.enemies.clear();
        self.hazards.reset();
        self.altars_used.clear();
        self.level_phase = LEVEL_COLLECT;
        self.collapse_elapsed_ticks = 0;
        self.collapse.clear();
        self.gift_platform.clear();
        self.event_friction_mul = 1.0;
        self.phase = PHASE_ENEMY_DRAFT;
        self.offer = Some(self.make_offer(content));
        self.shop = self.make_shop(content);
    }

    pub fn end(&mut self) {
        self.phase = PHASE_OVER;
        self.offer = None;
        self.shop = None;
    }

    /// Use the first unused altar within 2.0 units horizontally and 1.5 vertically of `feet`:
    /// apply one random upgrade and one random non-greater curse (run RNG) and record both in the

// ---- lines 700-760: Run::try_use_altar (curse application) ----
    /// apply one random upgrade and one random non-greater curse (run RNG) and record both in the
    /// RunInfo lists. Returns the applied (upgrade id, curse id), or None when nothing was used.
    pub fn try_use_altar(&mut self, content: &Content, feet: &Vec3) -> Option<(String, String)> {
        if self.phase != PHASE_PLAYING {
            return None;
        }
        let mut hit = None;
        for altar in &self.level.altars {
            if self.altars_used.contains(&altar.id) {
                continue;
            }
            let dx = feet.x - altar.pos.x;
            let dz = feet.z - altar.pos.z;
            let dy = (feet.y - altar.pos.y).abs();
            if dx * dx + dz * dz <= 4.0 && dy <= 1.5 {
                hit = Some(altar.id);
                break;
            }
        }
        let altar_id = hit?;
        if content.upgrades.is_empty() {
            return None;
        }
        // One random upgrade...
        let up_i = self.rng.below(content.upgrades.len());
        let upgrade_id = content.upgrades[up_i].id.clone();
        let upgrade_mods = content.upgrades[up_i].mods.clone();
        // ...and one random non-greater curse.
        let normal_curses: Vec<&crate::content::ModCard> = content.curses.iter().filter(|c| !c.greater).collect();
        if normal_curses.is_empty() {
            return None;
        }
        let cur_i = self.rng.below(normal_curses.len());
        let curse_id = normal_curses[cur_i].id.clone();
        let curse_mods = normal_curses[cur_i].mods.clone();
        let lives_before = self.mods.extra_lives;
        self.mods.apply(&upgrade_mods);
        self.lives = self
            .lives
            .saturating_add(self.mods.extra_lives.saturating_sub(lives_before));
        let lives_before = self.mods.extra_lives;
        self.mods.apply(&curse_mods);
        self.lives = self
            .lives
            .saturating_add(self.mods.extra_lives.saturating_sub(lives_before));
        self.upgrades.push(upgrade_id.clone());
        self.curses.push(curse_id.clone());
        self.altars_used.push(altar_id);
        Some((upgrade_id, curse_id))
    }

    fn normal_left(&self) -> usize {
        self.level
            .gifts
            .iter()
            .filter(|g| !self.gifts_taken.contains(&g.id))
            .count()
    }

    pub fn beacon_open(&self) -> bool {
        self.phase == PHASE_PLAYING && self.level_phase != LEVEL_COLLECT
