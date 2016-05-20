//
//  PutioTransfersButton.m
//  put.io adder
//
//  Created by Nico on 01/11/14.
//  Copyright (c) 2014 Nicolas Oelgart. All rights reserved.
//

#import "PutioTransfersButton.h"
#import "PutioHelper.h"
#import <Quartz/Quartz.h>

@implementation PutioTransfersButton

- (void)awakeFromNib
{
    NSTrackingArea *const trackingArea = [[NSTrackingArea alloc] initWithRect:NSZeroRect options:(NSTrackingMouseEnteredAndExited | NSTrackingMouseMoved | NSTrackingActiveAlways | NSTrackingInVisibleRect) owner:self userInfo:nil];
    [self addTrackingArea:trackingArea];
    [self.window invalidateCursorRectsForView:self];
}

- (void)resetCursorRects
{
    [super resetCursorRects];
    [self addCursorRect:self.bounds cursor:[NSCursor pointingHandCursor]];
}

- (void)mouseEntered:(NSEvent*)theEvent
{
    [super mouseEntered:theEvent];
    [[NSCursor pointingHandCursor] push];
}

- (void)mouseExited:(NSEvent*)theEvent
{
    [super mouseExited:theEvent];
    [[NSCursor pointingHandCursor] pop];
}

- (void)mouseUp:(NSEvent*)theEvent
{
    [super mouseUp:theEvent];
    CABasicAnimation *imageRotation = [CABasicAnimation animationWithKeyPath:@"transform.rotation"];
    bool isOpen = ![[PutioHelper sharedHelper].putioController transfersAreVisible];
    
    imageRotation.fromValue = [NSNumber numberWithFloat: isOpen ? -M_PI : 0];
    double to = isOpen ? 0 : M_PI;
    imageRotation.toValue = [NSNumber numberWithFloat:to];
    
    imageRotation.duration = 0.4;
    imageRotation.repeatCount = 1;

    CGRect frame = self.layer.frame;
    CGPoint center = CGPointMake(CGRectGetMidX(frame), CGRectGetMidY(frame));
    self.layer.position = center;
    self.layer.anchorPoint = CGPointMake(0.5, 0.5);
    
    [self.layer setValue:imageRotation.toValue forKey:imageRotation.keyPath];
    [self.layer addAnimation:imageRotation forKey:@"imageRotation"];

    self.layer.transform = CATransform3DMakeRotation(to, 0, 0.0, 1.0);
    [[[PutioHelper sharedHelper] putioController] toggleShowTransfers];
}

@end
