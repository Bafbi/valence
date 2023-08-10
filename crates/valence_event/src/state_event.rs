use std::{ops::{Deref, DerefMut}, marker::PhantomData, slice::{Iter, IterMut}, iter::Chain, collections::BTreeMap, cmp::Ordering, hash::Hasher, fmt};

use bevy_ecs::{prelude::Event, system::{Resource, SystemParam, Local, Res, ResMut}};
use bitfield_struct::bitfield;

#[bitfield(u8)]
pub struct EventState {
    pub canceled: bool,
    #[bits(7)]
    _pad: u8,
}

/// An `EventId` uniquely identifies an event stored in a specific [`World`].
///
/// An `EventId` can among other things be used to trace the flow of an event from the point it was
/// sent to the point it was processed.
///
/// [`World`]: crate::world::World
pub struct StateEventId<E: Event> {
    /// Uniquely identifies the event associated with this ID.
    // This value corresponds to the order in which each event was added to the world.
    pub id: usize,
    _marker: PhantomData<E>,
}

impl<E: Event> Copy for StateEventId<E> {}
impl<E: Event> Clone for StateEventId<E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E: Event> fmt::Display for StateEventId<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        <Self as fmt::Debug>::fmt(self, f)
    }
}

impl<E: Event> fmt::Debug for StateEventId<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "event<{}>#{}",
            std::any::type_name::<E>().split("::").last().unwrap(),
            self.id,
        )
    }
}

impl<E: Event> PartialEq for StateEventId<E> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<E: Event> Eq for StateEventId<E> {}

impl<E: Event> PartialOrd for StateEventId<E> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<E: Event> Ord for StateEventId<E> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}

#[derive(Debug)]
struct EventWithStateInstance<E: Event> {
    pub event_id: StateEventId<E>,
    pub event: E,
    pub state: EventState,
}

/// An event collection that represents the events that occurred within the last two
/// [`Events::update`] calls.
/// Events can be written to using an [`StateEventWriter`]
/// and are typically cheaply read using an [`EventWithStateReader`].
///
/// Each event can be consumed by multiple systems, in parallel,
/// with consumption tracked by the [`EventWithStateReader`] on a per-system basis.
///
/// If no [ordering](https://github.com/bevyengine/bevy/blob/main/examples/ecs/ecs_guide.rs)
/// is applied between writing and reading systems, there is a risk of a race condition.
/// This means that whether the events arrive before or after the next [`Events::update`] is unpredictable.
///
/// This collection is meant to be paired with a system that calls
/// [`Events::update`] exactly once per update/frame.
///
/// [`Events::update_system`] is a system that does this, typically initialized automatically using
/// [`add_event`](https://docs.rs/bevy/*/bevy/app/struct.App.html#method.add_event).
/// [`EventWithStateReader`]s are expected to read events from this collection at least once per loop/frame.
/// Events will persist across a single frame boundary and so ordering of event producers and
/// consumers is not critical (although poorly-planned ordering may cause accumulating lag).
/// If events are not handled by the end of the frame after they are updated, they will be
/// dropped silently.
///
/// # Example
/// ```
/// use bevy_ecs::event::{Event, Events};
///
/// #[derive(Event)]
/// struct MyEvent {
///     value: usize
/// }
///
/// // setup
/// let mut events = Events::<MyEvent>::default();
/// let mut reader = events.get_reader();
///
/// // run this once per update/frame
/// events.update();
///
/// // somewhere else: send an event
/// events.send(MyEvent { value: 1 });
///
/// // somewhere else: read the events
/// for event in reader.iter(&events) {
///     assert_eq!(event.value, 1)
/// }
///
/// // events are only processed once per reader
/// assert_eq!(reader.iter(&events).count(), 0);
/// ```
///
/// # Details
///
/// [`Events`] is implemented using a variation of a double buffer strategy.
/// Each call to [`update`](Events::update) swaps buffers and clears out the oldest one.
/// - [`EventWithStateReader`]s will read events from both buffers.
/// - [`EventWithStateReader`]s that read at least once per update will never drop events.
/// - [`EventWithStateReader`]s that read once within two updates might still receive some events
/// - [`EventWithStateReader`]s that read after two updates are guaranteed to drop all events that occurred
/// before those updates.
///
/// The buffers in [`Events`] will grow indefinitely if [`update`](Events::update) is never called.
///
/// An alternative call pattern would be to call [`update`](Events::update)
/// manually across frames to control when events are cleared.
/// This complicates consumption and risks ever-expanding memory usage if not cleaned up,
/// but can be done by adding your event as a resource instead of using
/// [`add_event`](https://docs.rs/bevy/*/bevy/app/struct.App.html#method.add_event).
///
/// [Example usage.](https://github.com/bevyengine/bevy/blob/latest/examples/ecs/event.rs)
/// [Example usage standalone.](https://github.com/bevyengine/bevy/blob/latest/crates/bevy_ecs/examples/events.rs)
///
#[derive(Debug, Resource)]
pub struct EventsWithState<E: Event> {
    /// Holds the oldest still active events.
    /// Note that a.start_event_count + a.len() should always === events_b.start_event_count.
    events_a: EventWithStateSequence<E>,
    /// Holds the newer events.
    events_b: EventWithStateSequence<E>,
    event_count: usize,
}

// Derived Default impl would incorrectly require E: Default
impl<E: Event> Default for EventsWithState<E> {
    fn default() -> Self {
        Self {
            events_a: Default::default(),
            events_b: Default::default(),
            event_count: Default::default(),
        }
    }
}

impl<E: Event> EventsWithState<E> {
    /// Returns the index of the oldest event stored in the event buffer.
    pub fn oldest_event_count(&self) -> usize {
        self.events_a
            .start_event_count
            .min(self.events_b.start_event_count)
    }
}

#[derive(Debug)]
struct EventWithStateSequence<E: Event> {
    events: Vec<EventWithStateInstance<E>>,
    start_event_count: usize,
}

// Derived Default impl would incorrectly require E: Default
impl<E: Event> Default for EventWithStateSequence<E> {
    fn default() -> Self {
        Self {
            events: Default::default(),
            start_event_count: Default::default(),
        }
    }
}

impl<E: Event> Deref for EventWithStateSequence<E> {
    type Target = Vec<EventWithStateInstance<E>>;

    fn deref(&self) -> &Self::Target {
        &self.events
    }
}

impl<E: Event> DerefMut for EventWithStateSequence<E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.events
    }
}

/// Reads events of type `T` in order and tracks which events have already been read.
#[derive(SystemParam, Debug)]
pub struct EventWithStateReader<'w, 's, E: Event> {
    reader: Local<'s, ManualEventWithStateReader<E>>,
    events: ResMut<'w, EventsWithState<E>>,
}

impl<'w, 's, E: Event> EventWithStateReader<'w, 's, E> {
    /// Iterates over the events this [`EventWithStateReader`] has not seen yet. This updates the
    /// [`EventWithStateReader`]'s event counter, which means subsequent event reads will not include events
    /// that happened before now.
    pub fn iter_mut(&mut self) -> ManualEventWithStateIterator<'_, E> {
        self.reader.iter_mut(&mut self.events)
    }

    /// Like [`iter`](Self::iter), except also returning the [`EventId`] of the events.
    pub fn iter_mut_with_id(&mut self) -> ManualEventWithStateIteratorWithId<'_, E> {
        self.reader.iter_mut_with_id(&mut self.events)
    }

    /// Determines the number of events available to be read from this [`EventWithStateReader`] without consuming any.
    pub fn len(&self) -> usize {
        self.reader.len(&self.events)
    }

    /// Returns `true` if there are no events available to read.
    ///
    /// # Example
    ///
    /// The following example shows a useful pattern where some behavior is triggered if new events are available.
    /// [`EventWithStateReader::clear()`] is used so the same events don't re-trigger the behavior the next time the system runs.
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// #
    /// #[derive(Event)]
    /// struct CollisionEvent;
    ///
    /// fn play_collision_sound(mut events: EventWithStateReader<CollisionEvent>) {
    ///     if !events.is_empty() {
    ///         events.clear();
    ///         // Play a sound
    ///     }
    /// }
    /// # bevy_ecs::system::assert_is_system(play_collision_sound);
    /// ```
    pub fn is_empty(&self) -> bool {
        self.reader.is_empty(&self.events)
    }

    /// Consumes all available events.
    ///
    /// This means these events will not appear in calls to [`EventWithStateReader::iter()`] or
    /// [`EventWithStateReader::iter_with_id()`] and [`EventWithStateReader::is_empty()`] will return `true`.
    ///
    /// For usage, see [`EventWithStateReader::is_empty()`].
    pub fn clear(&mut self) {
        self.reader.clear(&self.events);
    }
}

impl<'a, 'w, 's, E: Event> IntoIterator for &'a mut EventWithStateReader<'w, 's, E> {
    type Item = (&'a E, &'a mut EventState);
    type IntoIter = ManualEventWithStateIterator<'a, E>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// Sends events of type `T`.
///
/// # Usage
///
/// `StateEventWriter`s are usually declared as a [`SystemParam`].
/// ```
/// # use bevy_ecs::prelude::*;
///
/// #[derive(Event)]
/// pub struct MyEvent; // Custom event type.
/// fn my_system(mut writer: StateEventWriter<MyEvent>) {
///     writer.send(MyEvent);
/// }
///
/// # bevy_ecs::system::assert_is_system(my_system);
/// ```
///
/// # Limitations
///
/// `StateEventWriter` can only send events of one specific type, which must be known at compile-time.
/// This is not a problem most of the time, but you may find a situation where you cannot know
/// ahead of time every kind of event you'll need to send. In this case, you can use the "type-erased event" pattern.
///
/// ```
/// # use bevy_ecs::{prelude::*, event::Events};
/// # #[derive(Event)]
/// # pub struct MyEvent;
/// fn send_untyped(mut commands: Commands) {
///     // Send an event of a specific type without having to declare that
///     // type as a SystemParam.
///     //
///     // Effectively, we're just moving the type parameter from the /type/ to the /method/,
///     // which allows one to do all kinds of clever things with type erasure, such as sending
///     // custom events to unknown 3rd party plugins (modding API).
///     //
///     // NOTE: the event won't actually be sent until commands get applied during
///     // apply_deferred.
///     commands.add(|w: &mut World| {
///         w.send_event(MyEvent);
///     });
/// }
/// ```
/// Note that this is considered *non-idiomatic*, and should only be used when `StateEventWriter` will not work.
#[derive(SystemParam)]
pub struct StateEventWriter<'w, E: Event> {
    events: ResMut<'w, EventsWithState<E>>,
}

impl<'w, E: Event> StateEventWriter<'w, E> {
    /// Sends an `event`, which can later be read by [`EventWithStateReader`]s.
    ///
    /// See [`Events`] for details.
    pub fn send(&mut self, event: E) {
        self.events.send(event);
    }

    /// Sends a list of `events` all at once, which can later be read by [`EventWithStateReader`]s.
    /// This is more efficient than sending each event individually.
    ///
    /// See [`Events`] for details.
    pub fn send_batch(&mut self, events: impl IntoIterator<Item = E>) {
        self.events.extend(events);
    }

    /// Sends the default value of the event. Useful when the event is an empty struct.
    pub fn send_default(&mut self)
    where
        E: Default,
    {
        self.events.send_default();
    }
}

/// Stores the state for an [`EventWithStateReader`].
/// Access to the [`Events<E>`] resource is required to read any incoming events.
#[derive(Debug)]
pub struct ManualEventWithStateReader<E: Event> {
    last_event_count: usize,
    _marker: PhantomData<E>,
}

impl<E: Event> Default for ManualEventWithStateReader<E> {
    fn default() -> Self {
        ManualEventWithStateReader {
            last_event_count: 0,
            _marker: Default::default(),
        }
    }
}

#[allow(clippy::len_without_is_empty)] // Check fails since the is_empty implementation has a signature other than `(&self) -> bool`
impl<E: Event> ManualEventWithStateReader<E> {
    /// See [`EventWithStateReader::iter`]
    pub fn iter<'a>(&'a mut self, events: &'a mut EventsWithState<E>) -> ManualEventWithStateIterator<'a, E> {
        self.iter_with_id(events).without_id()
    }

    pub fn iter_mut<'a>(
        &'a mut self,
        events: &'a mut EventsWithState<E>,
    ) -> ManualEventWithStateIterator<'a, E> {
        self.iter_mut_with_id(events).without_id()
    }

    /// See [`EventWithStateReader::iter_with_id`]
    pub fn iter_with_id<'a>(
        &'a mut self,
        events: &'a mut EventsWithState<E>,
    ) -> ManualEventWithStateIteratorWithId<'a, E> {
        ManualEventWithStateIteratorWithId::new(self, events)
    }

    pub fn iter_mut_with_id<'a>(
        &'a mut self,
        events: &'a mut EventsWithState<E>,
    ) -> ManualEventWithStateIteratorWithId<'a, E> {
        ManualEventWithStateIteratorWithId::new(self, events)
    }

    /// See [`EventWithStateReader::len`]
    pub fn len(&self, events: &EventsWithState<E>) -> usize {
        // The number of events in this reader is the difference between the most recent event
        // and the last event seen by it. This will be at most the number of events contained
        // with the events (any others have already been dropped)
        // TODO: Warn when there are dropped events, or return e.g. a `Result<usize, (usize, usize)>`
        events
            .event_count
            .saturating_sub(self.last_event_count)
            .min(events.len())
    }

    /// Amount of events we missed.
    pub fn missed_events(&self, events: &EventsWithState<E>) -> usize {
        events
            .oldest_event_count()
            .saturating_sub(self.last_event_count)
    }

    /// See [`EventWithStateReader::is_empty()`]
    pub fn is_empty(&self, events: &EventsWithState<E>) -> bool {
        self.len(events) == 0
    }

    /// See [`EventWithStateReader::clear()`]
    pub fn clear(&mut self, events: &EventsWithState<E>) {
        self.last_event_count = events.event_count;
    }
}

/// An iterator that yields any unread events from an [`EventWithStateReader`] or [`ManualStateEventReader`].
pub struct ManualEventWithStateIterator<'a, E: Event> {
    iter: ManualEventWithStateIteratorWithId<'a, E>,
}

impl<'a, E: Event> Iterator for ManualEventWithStateIterator<'a, E> {
    type Item = (&'a E, &'a mut EventState);
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(event, state, _)| (event, state))
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.iter.nth(n).map(|(event, state, _)| (event, state))
    }

    fn last(self) -> Option<Self::Item>
    where
        Self: Sized,
    {
        self.iter.last().map(|(event, state, _)| (event, state))
    }

    fn count(self) -> usize {
        self.iter.count()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, E: Event> ExactSizeIterator for ManualEventWithStateIterator<'a, E> {
    fn len(&self) -> usize {
        self.iter.len()
    }
}

/// An iterator that yields any unread events (and their IDs) from an [`EventWithStateReader`] or [`ManualStateEventReader`].
#[derive(Debug)]
pub struct ManualEventWithStateIteratorWithId<'a, E: Event> {
    reader: &'a mut ManualEventWithStateReader<E>,
    chain: Chain<IterMut<'a, EventWithStateInstance<E>>, IterMut<'a, EventWithStateInstance<E>>>,
    unread: usize,
}

impl<'a, E: Event> ManualEventWithStateIteratorWithId<'a, E> {
    /// Creates a new iterator that yields any `events` that have not yet been seen by `reader`.
    pub fn new(reader: &'a mut ManualEventWithStateReader<E>, events: &'a mut EventsWithState<E>) -> Self {
        let a_index = (reader.last_event_count).saturating_sub(events.events_a.start_event_count);
        let b_index = (reader.last_event_count).saturating_sub(events.events_b.start_event_count);
        let a = events.events_a.get_mut(a_index..).unwrap_or_default();
        let b = events.events_b.get_mut(b_index..).unwrap_or_default();

        let unread_count = a.len() + b.len();
        // Ensure `len` is implemented correctly
        // debug_assert_eq!(unread_count, reader.len(events));
        reader.last_event_count = events.event_count - unread_count;
        // Iterate the oldest first, then the newer events
        let chain = a.iter_mut().chain(b.iter_mut());

        Self {
            reader,
            chain,
            unread: unread_count,
        }
    }

    /// Iterate over only the events.
    pub fn without_id(self) -> ManualEventWithStateIterator<'a, E> {
        ManualEventWithStateIterator { iter: self }
    }
}

impl<'a, E: Event> Iterator for ManualEventWithStateIteratorWithId<'a, E> {
    type Item = (&'a E, &'a mut EventState, StateEventId<E>);
    fn next(&mut self) -> Option<Self::Item> {
        match self
            .chain
            .next()
            .map(|instance| (&instance.event, &mut instance.state, instance.event_id))
        {
            Some(item) => {
                // detailed_trace!("EventWithStateReader::iter() -> {}", item.1);
                self.reader.last_event_count += 1;
                self.unread -= 1;
                Some(item)
            }
            None => None,
        }
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        if let Some(EventWithStateInstance { event_id, event , state}) = self.chain.nth(n) {
            self.reader.last_event_count += n + 1;
            self.unread -= n + 1;
            Some((event, state, *event_id))
        } else {
            self.reader.last_event_count += self.unread;
            self.unread = 0;
            None
        }
    }

    fn last(self) -> Option<Self::Item>
    where
        Self: Sized,
    {
        let EventWithStateInstance { event_id, event, state } = self.chain.last()?;
        self.reader.last_event_count += self.unread;
        Some((event, state, *event_id))
    }

    fn count(self) -> usize {
        self.reader.last_event_count += self.unread;
        self.unread
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.chain.size_hint()
    }
}

impl<'a, E: Event> ExactSizeIterator for ManualEventWithStateIteratorWithId<'a, E> {
    fn len(&self) -> usize {
        self.unread
    }
}

impl<E: Event> EventsWithState<E> {
    /// "Sends" an `event` by writing it to the current event buffer. [`EventWithStateReader`]s can then read
    /// the event.
    pub fn send(&mut self, event: E) {
        let event_id = StateEventId {
            id: self.event_count,
            _marker: PhantomData,
        };
        // detailed_trace!("Events::send() -> id: {}", event_id);

        let event_instance = EventWithStateInstance { event_id, event, state: EventState::new() };

        self.events_b.push(event_instance);
        self.event_count += 1;
    }

    /// Sends the default value of the event. Useful when the event is an empty struct.
    pub fn send_default(&mut self)
    where
        E: Default,
    {
        self.send(Default::default());
    }

    /// Gets a new [`ManualStateEventReader`]. This will include all events already in the event buffers.
    pub fn get_reader(&self) -> ManualEventWithStateReader<E> {
        ManualEventWithStateReader::default()
    }

    /// Gets a new [`ManualStateEventReader`]. This will ignore all events already in the event buffers.
    /// It will read all future events.
    pub fn get_reader_current(&self) -> ManualEventWithStateReader<E> {
        ManualEventWithStateReader {
            last_event_count: self.event_count,
            ..Default::default()
        }
    }

    /// Swaps the event buffers and clears the oldest event buffer. In general, this should be
    /// called once per frame/update.
    pub fn update(&mut self) {
        std::mem::swap(&mut self.events_a, &mut self.events_b);
        self.events_b.clear();
        self.events_b.start_event_count = self.event_count;
        debug_assert_eq!(
            self.events_a.start_event_count + self.events_a.len(),
            self.events_b.start_event_count
        );
    }

    /// A system that calls [`Events::update`] once per frame.
    pub fn update_system(mut events: ResMut<Self>) {
        events.update();
    }

    #[inline]
    fn reset_start_event_count(&mut self) {
        self.events_a.start_event_count = self.event_count;
        self.events_b.start_event_count = self.event_count;
    }

    /// Removes all events.
    #[inline]
    pub fn clear(&mut self) {
        self.reset_start_event_count();
        self.events_a.clear();
        self.events_b.clear();
    }

    /// Returns the number of events currently stored in the event buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.events_a.len() + self.events_b.len()
    }

    /// Returns true if there are no events currently stored in the event buffer.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Creates a draining iterator that removes all events.
    pub fn drain(&mut self) -> impl Iterator<Item = E> + '_ {
        self.reset_start_event_count();

        // Drain the oldest events first, then the newest
        self.events_a
            .drain(..)
            .chain(self.events_b.drain(..))
            .map(|i| i.event)
    }

    /// Iterates over events that happened since the last "update" call.
    /// WARNING: You probably don't want to use this call. In most cases you should use an
    /// [`EventWithStateReader`]. You should only use this if you know you only need to consume events
    /// between the last `update()` call and your call to `iter_current_update_events`.
    /// If events happen outside that window, they will not be handled. For example, any events that
    /// happen after this call and before the next `update()` call will be dropped.
    pub fn iter_current_update_events(&self) -> impl ExactSizeIterator<Item = &E> {
        self.events_b.iter().map(|i| &i.event)
    }

    /// Get a specific event by id if it still exists in the events buffer.
    pub fn get_event(&self, id: usize) -> Option<(&E, StateEventId<E>)> {
        if id < self.oldest_id() {
            return None;
        }

        let sequence = self.sequence(id);
        let index = id.saturating_sub(sequence.start_event_count);

        sequence
            .get(index)
            .map(|instance| (&instance.event, instance.event_id))
    }

    /// Oldest id still in the events buffer.
    pub fn oldest_id(&self) -> usize {
        self.events_a.start_event_count
    }

    /// Which event buffer is this event id a part of.
    fn sequence(&self, id: usize) -> &EventWithStateSequence<E> {
        if id < self.events_b.start_event_count {
            &self.events_a
        } else {
            &self.events_b
        }
    }
}

impl<E: Event> std::iter::Extend<E> for EventsWithState<E> {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = E>,
    {
        let old_count = self.event_count;
        let mut event_count = self.event_count;
        let events = iter.into_iter().map(|event| {
            let event_id = StateEventId {
                id: event_count,
                _marker: PhantomData,
            };
            event_count += 1;
            EventWithStateInstance { event_id, event, state: EventState::new() }
        });

        self.events_b.extend(events);

        if old_count != event_count {
            // detailed_trace!(
            //     "Events::extend() -> ids: ({}..{})",
            //     self.event_count,
            //     event_count
            // );
        }

        self.event_count = event_count;
    }
}