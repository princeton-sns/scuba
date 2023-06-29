# NoiseKV Design Thoughts/Decisions

## Linked devices and Contacts

Initially, the notion of contacts was baked into the data abstraction libraries
because it was a common-enough application feature that it was easier to implement
once for all applications. However, embedding contacts in the data abstraction
library meant that it was more difficult for contacts to leverage the existing
groups/permissions mechanisms, since it was treated as a "special" instance of
such structures. This not only makes contacts less flexible (e.g. limiting sharing
to contacts imposes additional constraints on the overall policy structure) but
also misses the opportunity for employing economy of mechanism, since we have two
separate code paths (contacts vs all other groups) that kind of do the same thing.

I am now in the process of trying to remove the notion of contacts from noise-kv,
and instead set contacts up as any other group would be set up from the application
end. What makes this interesting is the unique access control structure that contacts 
have.

### Permissions

First, let me summarize the permissions structure currently implemented
by the library: "readers" are devices on which the groups + data are stored, "writers"
are devices that can modify data, and "admins" are devices that can modify groups.
"writers" also contain "reader" privileges, and "admins" also contain "writer" 
privileges. This choice was made to simplify the implementation, but precludes
permissions levels that are not strict supersets of each other, e.g. having the
permissions to modify groups without the permission to modify data. This is not
a fundamental limitation, however, and can be implemented in a different version 
of the data abstraction layer.

### Linked groups

Before jumping into contacts, let's briefly go over what a user's linked device
grouping looks like, and how it changes. This ideally will be very similar to
contact data.

The top-level group ("devices") contains pointers to every distinct "linked" subgroup
the device has access to (including its own). Say the current device's linked 
subgroup has id "A", and there are two devices: "A1" and "A2". Say the current
device is "A1". The "devices" group will point to group "A", which will point to 
both device "A1" and "A2". "A" has "admin" access to "devices", "A", "A1", and 
"A2", meaning that each individual device has "admin" access to all of "devices",
"A", "A1", and "A2".

TODO does every node need to have its own permissions object? Or can permissions
trickle down the tree? Or is there a mix -> trickling down unless one is specified
at a particular point? If trickling down or mixing, only one permissions object is 
needed at this point (a - ["A"], w - ["A"], r - ["A"]). Otherwise, three identical 
permissions objects are needed for each node in the current tree.

If the user associated with "A" wants to add a third device, it must do so via
in existing device, i.e. either "A1" or "A2". The third device, let's assign it
id "A3", asks for permission to be added by one of the existing devices, let's say 
"A1" (via, e.g., a QR code). If "A1" rejects it, nothing changes. 

If "A1" accepts it, it should act as any other group membership change (TODO remove 
"linked group" notion from library as well, but only after contacts). Currently, 
"A1" will send "A3" the existing linked group info so its state is synced with "A1"
and "A2", and then "A1" adds "A3" as a child of "A" (which gets propagated to "A1",
"A2", and "A3"). Then "A1" updates the permissions set to also include "A3". All 
of these operations are valid since "A1" has "admin" permissions for group "A".

### Contacts

Ok, now let's talk about contacts. Say another distinct set of linked devices has
id "B", with individual devices being "B1" and "B2". The top-level "devices" group
on each of these two devices points to "B", which then points to both "B1" and
"B2". Say "B" wants to add "A" as a contact. For simplicity, let's assume that
the contact relationship is bidirectional, i.e. if "B" adds "A" as a contact then
"A" will also have "B" as a contact. This isn't so much "contacts" as we know
them on our phones, but more of a mutual identification/discovery mechanism such
that operations can be properly encrypted and routed to the intended device(s).

TODO need to better understand contact discovery/how one "finds" new contacts.

Assuming "B1" has already "found" "A1" (where the ids of the devices double as 
public keys), "B1" can send a contact request to "A1" (in the implementation, "B"'s
device group is piggybacked along with this request). If "A1" rejects the request,
nothing changes.

If "A1" accepts the request, it first adds "B"'s subgroup to its top-level "devices" 
group, and then sends its own subgroup back to "B1". The goal is that each "linked"
subgroup will be kept in sync with its originator should its linked group membership
change. For instance, if "B3" becomes added to "B"'s subgroup by "B1" (as "A3"
was added by "A1" above), that membership change will be propagated to all of
"A"'s devices in addition to all of "B"'s devices, such that any future operations
between the two linked groups go to all linked devices. Similarly, if the user 
associated with group "B" chooses to unlink "B2", operations between "A" and "B" 
should no longer go to the device previously identified as "B2".

Now that we've established the goal and the high-level process of adding a contact,
let's peer into the details and reason about how the associated permissions of
each group mitigate malicious behavior between devices that do not have a contact
relationship.

When "A1" adds "B"'s subgroup to its top-level "devices" group, this happens in
several steps. "A1" first adds, as a child to "devices", id "B" (this can also happen
after the next step, and may in fact be better after the next step so there isn't 
a dangling pointer although these two steps should happen atomically). Recall that 
"A1" has "admin" permissions on the "devices" group, so it can modify its structure.
This change will propagate to all of "A"'s devices (since they are all admins and
thus readers on the "devices" group).

Then, since "A1" has "B"'s subgroup (recall it was piggybacked with the request),
"A1" should store "B"'s subgroup. Before sending the initial contact request, however,
"B" should have given "A" read permissions to its subtree such that any changes
to it are propagated. Adding "A" as a reader should also trigger the initial propagation
such that a special "add-contact" method doesn't need to exist (anymore). Similarly,
once "A" accepts the request, it should add "B" as a reader to its linked group,
which sends "A"s linked group to "B" and also any future updates to it.

However, imagine now that another user "C" wants to add "A" as a contact. By adding
"C" as a reader to "A"s linked group, "C" may be able to see that "A" has shared
its information with "B", and "B" may see the same about "C". A new permission type 
may fix this (we could call it a "reader-mod-readers" permissions level for now), 
where the list of readers is not propagated but everything else is.

Assuming the above issue is solved, if two devices, each from distinct linked groups, 
do not have a contact relationship, they should not have any knowledge about the
others linked groups. Once those devices do have a contact relationship, they
cannot modify the others linked group membership in any meaningful way, because 
the permissions checks on all devices that will receive those operations will fail.
A corrupted device may circumvent those permissions checks locally, but it cannot
force the remaining devices to circumvent their own local permissions checks. Thus
a malicious device cannot corrupt the state on other, non-malicious devices.

### Application-level API

Although a special "add-contact" method in the library was a bit redundant, it 
exposed an intuitive application-level API. Making "contacts" just like any other
group _could_ be confusing, but let's walk through how it would look to the app
and revisit this question again afterwards.

Let's assume that the current device maps to "A1" under the hood, that the linked
group id is "A", that the linked group also contains "A2" and "A3", and that we
currently have no "contacts" added. Another linked device set, "B" with devices
"B1" and "B2", also has no "contacts" added.

If the current device wants to add all of "B" as a contact, it first needs to send
a request to "B1". As mentioned above, this "request" can be sent by giving "B1"
"reader-mod-readers" permission, which will propagate "A"s linked subtree and the
associated permissions (modulo the "readers") to "B1". However, at this point, 
_only_ "B1" has been given access to "A"'s linked subtree, so it's state has now
diverged from the rest of its linked devices (in this case, "B2"). So simply
granting access in this way initially is not enough for discovering all of "B"'s
devices. "A" needs to know of all "B"s devices _before_ it gives _"B"_ "reader-mod-readers"
permission on its linked subtree in order to keep all of "B"'s devices in sync. 
Initially, NoiseKV essentially relied on this step _to_ discover the rest of "B"'s
devices, but if we want to leverage economy of mechanism and trigger contact additions
by merely "sharing" linked subtree data, an additional contact-discovery mechanism
is needed before this step is performed.

### Contact discovery

NoiseKV could implement its own simple form of contact discovery once _one_ device's
id (from a distinct linked subtree) is known. NoiseKV does not solve that yet, and
assumes that devices "just know" one device id from a linked subtree that they want
to connect with. 

With this assumption in mind, NoiseKV currently naively just "asks" "B1" for all of
its linked devices on "A1"'s behalf, and then creates its own copy of "B"'s linked
group on each of "A"'s devices (and vice versa), via it's special "add-contact"
method. However, this is what forces NoiseKV to implement additional, redundant logic
when a device's linked subtree is updated - since it is a copy, and not a shared
object, NoiseKV must track and propagate changes to all contacts explicitly when a 
device's linked subtree changes, through a different mechanism than that of shared 
objects (which we are now trying to use).

Repurposing the initial step of "ask"ing "B1" for all its linked device information
on "A1"'s behalf, and then "sharing" "A"'s linked subtree with all "B" should work.
But we are still operating under the assumption that "A1" knows one of "B"'s device
ids, which doesn't seem trivial. How do other contact discovery mechanisms bootstrap
this process?

Based on the below, it seems like ultimately public keys need to be associated 
with some other third-party identifier (email, phone number, social media, etc),
at which point some private contact discovery mechanism can compute an overlap
of identifiers that are registered with the system and return those to the user's
device(s). Thus, our initial assumption that a device knows one of the ids of another 
device it wants to contact would, indeed, be realistic. The current device would
just ask for the Noise-public key of another user whose email or phone they have,
and then proceed with future operations using that public key.

#### [Signal contact discovery](https://signal.org/blog/private-contact-discovery/)

The first thing to notice about Signal's contact discovery mechanism is that it
uses the preexisting social graph of phone numbers in a user's address book. This
is unlike the setting that Noise operates in, since device public keys will be
newly assigned to any device entering the system. Ideally, Noise will not be limited
by any existing social graphs, although it may want to leverage them. (This does
start to sound like federated identity servers, e.g. Matrix - below).

#### [Matrix identity server](https://matrix.org/docs/legacy/faq/)

Uses email addresses or phone numbers (e.g. "third-party identifiers" or 
[3PIDs](https://spec.matrix.org/unstable/identity-service-api/)).
Identity links this 3PID to user's id (forming "matrix identifiers" or MXIDs).
However, it is still unclear which parts of this identity are required vs optional,
and thus which ones are ultimately used for contact discovery.

[This](https://spec.matrix.org/latest/#identity) seems to say that 3PIDs are
optional, and that at the bare minimum a user can just have a Matrix user ID
(to which they can later link other 3PIDs). If a user just has a Matrix user ID,
though, how would that user contact any other user?

Note that identity servers seem to be the ones that track this "linking" of IDs,
and are thus responsible for looking up users via 3PIDs. But can users be looked
up via MXIDs? Although not explicity, my impression is that the ethod is similar
to that of Signal's contact discovery, i.e. leveraging pre-existing social
networks rather than creating entirely new ones.

### Application-level API cont.

Initially, because we had a special "add-contact" method, "A1" would send a request
to "B1" rather than all of "B"'s devices. "B1" would then orchestrate synchronization
across its other linked devices, respond to "A1", and "A1" would also orchestrate
synchronization across its linked devices (i.e. via a special contacts control path).
But, because contact discovery should give "A1" _all_ of "B"'s linked subtree,
effectively, "A1" should send the request to _all_ of "B", at which only one of
"B"'s devices needs to confirm/deny the request.

We mentioned previously that this "request" may just look like device "A1" adding 
"B" as a "reader-mod-readers" of "A"'s linked subtree. But how can this sharing
then be confirmed or denied? This property is unlike other kinds of sharing, because
the protocol asks the sharee for information as well (it is an exchange), unlike
only giving the sharee some information (adding them to a group, after which point
the sharee can continuously decide which information it wants to share or not, e.g.
by messaging in the group or not).

So, not only does sharing need to be prefaced by contact discovery, but also by
a unique operation that allows the sharee to accept or deny the potential contact
connection. Once that is accepted, then the associated devices should be free to
simply share their linked subtrees with one another.

To avoid implementing contact discovery for now, we can continue to use the initial
mechanism that we have been using: "A1" just knows "B1"'s public key, then "A1" 
sends its own linked subtree to "B1" as a "trade" via a special operation that first
requires "B1" to confirm/deny the trade. If "B1" denies, nothing changes. If "B1"
confirms, it updates its own store with "A"'s linked subtree, then adds "A" as
a "reader-mod-readers" to its own "B" linked subtree (at which point all of "B"'s
devices should have "A" added, "A" has all of "B"'s linked subtree added).

At this point, "A" still needs to add "B" to its linked subtree (it initially only
sent "B" a copy, such that "B" knew how to add "A"). Because "B" just shared its
linked subtree with "A", "A" can just give "A" "reader-mod-readers" permissions 
on its linked subtree, and be done. But this adds a bit of redundancy, still, 
since "A" essentially sent its linked subtree twice. I may have tried to pre-optimize
with the piggybacking, so let's simplify the data exchange without trying to
minimize the number of round trips first.o

Upon writing out the overly simple version, I realized that the first "send"
of the linked subtree is essentially the replacement of _contact discovery_, and
the second is what happens during _contact addition_. So the redundancy here only
exists because we are running a version of contact discovery within the contact
addition workflow.

#### Naive contact addition protocol (with contact discovery workaround)

**Two** operations for a DENY and **eight** operations for a CONFIRM.

1. "A1" sends "B1" a request to become contacts.

1. "B1" either confirms or denies (via reply to "A1").

1. If "A1" receives confirmation, "A1" asks "B1" for linked subtree.

1. "B1" sends "B" subtree to "A1".

1. "A1" stores "B" subtree and adds "B" as a "reader-mod-readers" on its linked 
subtree.

1. "A1" then sends another message to "B" saying "B" should also add "A".
(TODO to decide: does "A1" adding "B" here propagate to all of "A"'s devices?)

1. "B" stores "A" subtree and also receives the following special message, triggering
"B" to add "A" as "reader-mod-readers" on its linked subtree.

1. "A" receives "B" linked subtree (TODO if "B" subtree was initially propagated
to all of "A"'s devices, then nothing happens, otherwise it gets propagated here).

#### Slightly optimized contact addition protocol (with contact discovery workaround)

**Two** operations for a DENY and **six** operations for a CONFIRM.

1. "A1" sends "B1" a request to become contacts, with its "A" linked subtree 
piggybacked.

1. "B1" either confirms or denies. If "B1" denies the request, it can simply
ignore it or reply back to "A1".

1. If "B1" confirms, it first adds "A" as "reader-mod-readers" to its "B" subtree.

1. Then "B1" sends a message to "A" saying "A" should also add "B".
(TODO to decide: does "B1" adding "A" here propagate to all of "B"'s devices?)

1. "A" stores "B"'s linked subtree and receives the following special message, 
triggering "A" to add "B" as "reader-mod-readers" on its linked subtree.

1. "B" receives "A" linked subtree (TODO if "A" subtree was initially propagated
to all of "B"'s devices, then nothing happens, otherwise it gets propagated here).

### Summary
