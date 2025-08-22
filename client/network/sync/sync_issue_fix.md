# Sync issue fix

## How chain syncing works

When a new peer connects to the network it starts syncing blocks from bootnodes and other peers it can detect

It requests chunks of 64 blocks from peers, and peers respond with a number of blocks that is <= the number of requested.

The sync engine then takes note of which blocks were received, and makes successive requests to fetch the rest

This works similar to bit torrent - it has a map of blocks it wants, and will keep making requests until it has them all

It will fill gaps left by partially serviced requests. 

This is all built in, and from observation, works very well. 

## Cause of the sync issue

There's two causes for the sync issue - one on the local peer, which bans peers after a single timeout, for 60 seconds. 

The other is on the remote peer, which bans our peer when we request the same block request more than 2 times (hard coded number 
but we could change this or make it configurable if we need to)

This happens on bigger blocks and slow networks. The max block bytes value on the server is set to 8 MB - when the blocks total more than that, the server
only sends the first 8MB worth of blocks. The client, as explained above, keeps track of which blocks it has, and starts pulling the rest after this. 

In our experience though, the large request on a slow connection tends to time out which leads to mutual bans of local and remote peer. 

## Reason for fast peer purging

We don't really know why the peers are extremely diligent on killing connections to bad peers - they even dial in reputation changes. 

But I think we can safely disable some of these systems on a new peer which isn't even close to the top of the chain

## Normal operations mode considerations! 

Importantly, the sync engine kicks in during normal operations, whenever we fall too far behind. So fast banning is important to keep the 
network efficient. As it does not just happen on initial sync but any time we fall behind enough blocks. 

We can therefore not just increase all the timeouts, and remove these precautions

## Intended Fix

The intended fix for this was to go slow until it's safe to go fast again

On a request that times out, we would then go slow until the max block this request was trying to get, then ramp max blocks back up 
to 64. Oscillating between 1 and 64 minimizes the risk that we get more timeouts and experience mutual bans. Vs for example slowly ramping up
and down, which would cause many more timeouts, and lead to peers reporting each other, reducing reputation, and timeouts, and backoffs. 

The goal is to avoid timeouts as much as possible. 

So here is how it works
1. Pull 64 blocks at a time (or whatver max-blocks-per-request is set to - this is a parameter on the node)
2. On detecting the first timeout, go into slow mode where we set max blocks to 1
3. Once we have covered the entire range of the bad request, we move back to fast mode


## Accidental fix

There were some code errors trying to implement this leading to this behavior:

1. On the first timeout, we reduce the max block size by 1 and try again
2. We keep doing this, we never ban the other peer during this time, and we keep requesting 1 less every time. 
3. This means we have up to 64 requests that are - importantly - all different from one another
4. Once we have the blocks in the range, we reset to max-blocks-per-request as above

With this setting, the client was blasting through the timeouts, recovered, and kept going pretty fast. 

The reason this fix works better than the intended fix is that we keep making _different_ requests, so we never get banned by the 
remote peer for making the same request too often! This way we sail through, and worst case we request 1 eventually, same as 
the intended fix. 
