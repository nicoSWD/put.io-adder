//
//  PutioBrowser.h
//  put.io adder
//
//  Created by Nico on 7/4/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import <Cocoa/Cocoa.h>
#import <WebKit/WebKit.h>

@interface PutioBrowser : NSWindowController
{
    IBOutlet WebView *webView;
}

@property (nonatomic, retain) WebView *webView;

@end
